# Scema Cortex :: top-k cosine similarity, SIMD kernel
#
# Compiled to a shared library and loaded over ctypes by kernels/__init__.py:
#
#     mojo build --emit shared-lib similarity.mojo -o libscemasim.so
#
# Rows of `matrix` and the `query` are expected L2-normalised by the caller,
# so a dot product is already cosine similarity and no division is needed.
#
# Targets Mojo v1.0.0 (unified `Pointer`; `UnsafePointer` is deprecated).
#
# NOTE: this file has NOT been compiled on the development machine -- Modular
# ships no native Windows toolchain, so it is built under WSL (see README.md).
# If `@export` or the `Pointer` spelling has drifted in your Mojo release,
# those two surfaces are the only places to adjust; the numerics are plain
# arithmetic. The Python side degrades to the torch/numpy kernel and produces
# identical results either way, so a build failure here is a throughput
# regression, never a correctness one.

from sys.info import simdwidthof
from algorithm import vectorize
from memory import Pointer

alias dtype = DType.float32
alias simd_width = simdwidthof[dtype]()


fn dot(a: Pointer[mut=False, Scalar[dtype]], b: Pointer[mut=False, Scalar[dtype]], d: Int) -> Float32:
    """SIMD dot product over `d` contiguous floats."""
    var acc = SIMD[dtype, simd_width](0)

    @parameter
    fn accumulate[width: Int](offset: Int):
        var av = a.load[width=width](offset)
        var bv = b.load[width=width](offset)
        # Widen narrow tails back into the accumulator lane layout.
        @parameter
        if width == simd_width:
            acc += rebind[SIMD[dtype, simd_width]](av * bv)
        else:
            var prod = av * bv
            for i in range(width):
                acc[0] += prod[i]

    vectorize[accumulate, simd_width](d)
    return acc.reduce_add()


@export
fn topk_cosine(
    query: Pointer[mut=False, Scalar[dtype]],
    matrix: Pointer[mut=False, Scalar[dtype]],
    n: Int32,
    d: Int32,
    k: Int32,
    out_idx: Pointer[mut=True, Scalar[DType.int32]],
    out_score: Pointer[mut=True, Scalar[dtype]],
):
    """Write the indices and scores of the `k` rows most similar to `query`.

    Maintains a sorted top-k buffer by insertion. For the k <= 32 this system
    uses, that beats a full sort: one pass over n rows, and the common case is
    a single comparison against the running minimum.
    """
    var rows = Int(n)
    var dim = Int(d)
    var keep = Int(k)
    if keep > rows:
        keep = rows

    # Initialise the buffer to "worse than anything" -- cosine is >= -1.
    for i in range(keep):
        out_idx[i] = -1
        out_score[i] = -2.0

    for row in range(rows):
        var score = dot(query, matrix.offset(row * dim), dim)

        # Cheap rejection: the buffer is sorted descending, so the last slot
        # is the weakest survivor.
        if score <= out_score[keep - 1]:
            continue

        # Shift the tail down and drop the insertion point in place.
        var pos = keep - 1
        while pos > 0 and out_score[pos - 1] < score:
            out_score[pos] = out_score[pos - 1]
            out_idx[pos] = out_idx[pos - 1]
            pos -= 1
        out_score[pos] = score
        out_idx[pos] = Int32(row)
