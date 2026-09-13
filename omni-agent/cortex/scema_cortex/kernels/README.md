# Cortex kernels

`topk_cosine` is the only O(n·d) inner loop in Scematica Omni-Agent. Every sense-loop
cycle scores each candidate for **novelty** — one top-k cosine search against
the whole memory matrix — so it gets a swappable implementation.

| Kernel  | Status on this machine | Notes |
|---------|------------------------|-------|
| `mojo`  | **not active** | No native Windows toolchain exists for Mojo. Build under WSL (below). |
| `numpy` | **active** | `argpartition`, CPU. Measured fastest here. |
| `torch` | opt-in | GPU matmul + `topk`. Measured **0.24x** (slower) -- see below. |

Selection is automatic (mojo → numpy). Override with
`SCEMA_KERNEL=mojo|torch|numpy`.

All three return identical results. A missing or broken Mojo artifact is a
throughput regression, never a correctness one — `kernels/__init__.py` catches
the failure and falls through.

## Why the GPU is not the fast path here

Measured on this machine (`python -m scema_cortex.bench`, d=384, k=8):

```
        n         numpy         torch   agree
    1,000      0.040ms      0.520ms   yes
   10,000      0.958ms      3.110ms   yes
  100,000      8.160ms     28.284ms   yes
at n=100,000: torch is 0.24x SLOWER than numpy
```

The GPU loses because every call copies the entire memory matrix
host→device — memory rows are owned by numpy on the host — and that copy
dwarfs the matmul it enables. At the sizes this agent actually reaches
(thousands of rows, sub-millisecond in numpy) retrieval is not the bottleneck.
The GPU earns its place on batched TasteNet inference and training instead,
where the data is already resident.

This is also the case *for* the Mojo kernel: it works in-place on the host
buffer with no copy and no allocation, which is exactly the cost that beats
the GPU here.

Re-run the bench before believing any of this on different hardware.

## Building the Mojo kernel (WSL)

Modular ships Mojo for Linux and macOS only. On Windows, build inside WSL and
drop the `.so` next to the source — the loader finds it by name.

```bash
# In WSL (Ubuntu-22.04 is already installed on this machine)
curl -fsSL https://pixi.sh/install.sh | bash
exec $SHELL
cd /mnt/c/Users/deads/OneDrive/Documents/AGI/omni-agent/cortex/scema_cortex/kernels
pixi init . -c https://conda.modular.com/max-nightly/ -c conda-forge
pixi add modular
pixi run mojo build --emit shared-lib similarity.mojo -o libscemasim.so
```

Then, from Windows:

```powershell
python -c "from scema_cortex.kernels import active_kernel; print(active_kernel())"
```

It should print `mojo`. If it prints `torch`, the artifact was not found or
not loadable — check `SCEMA_USE_MOJO=1` and that the file is named
`libscemasim.so`, `scemasim.dll`, or `libscemasim.dylib`.

### If the build errors

`similarity.mojo` has not been compiled on this machine, so treat the build as
unverified. Two surfaces are version-sensitive and are the only likely
failures:

1. **`@export`** — the decorator that gives the function a C ABI symbol. The
   v1.0.0 changelog confirms `mojo build --emit shared-lib` but does not
   document the decorator spelling; check
   <https://docs.modular.com/mojo/manual/c-ffi/>.
2. **`Pointer[mut=False, Scalar[dtype]]`** — v1.0.0 unified `Pointer` and
   `UnsafePointer` and moved unsafe operations behind `unsafe_` prefixes. Older
   releases want `UnsafePointer[Float32]`.

The arithmetic in between is plain arithmetic. The ctypes signature the loader
expects is fixed, and changing it means editing `_topk_mojo` to match:

```c
void topk_cosine(const float* query, const float* matrix,
                 int n, int d, int k,
                 int* out_idx, float* out_score);
```
