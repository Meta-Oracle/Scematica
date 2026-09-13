"""Runtime configuration for the Scema Cortex.

Every knob is environment-overridable so the TypeScript side, tests and the
CLI can all drive the same process without editing code.
"""
from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path


def _env_bool(name: str, default: bool) -> bool:
    raw = os.getenv(name)
    if raw is None:
        return default
    return raw.strip().lower() in {"1", "true", "yes", "on"}


def _env_int(name: str, default: int) -> int:
    try:
        return int(os.getenv(name, ""))
    except ValueError:
        return default


def _env_float(name: str, default: float) -> float:
    try:
        return float(os.getenv(name, ""))
    except ValueError:
        return default


REPO_ROOT = Path(__file__).resolve().parents[2]


@dataclass
class CortexConfig:
    # --- storage ---
    data_dir: Path = field(
        default_factory=lambda: Path(
            os.getenv("SCEMA_DATA_DIR", str(REPO_ROOT / "data" / "cortex"))
        )
    )

    # --- embeddings ---
    # "auto" resolves: local sentence-transformers -> OpenAI API -> hashing.
    embed_backend: str = field(default_factory=lambda: os.getenv("SCEMA_EMBED_BACKEND", "auto"))
    embed_model: str = field(
        default_factory=lambda: os.getenv("SCEMA_EMBED_MODEL", "sentence-transformers/all-MiniLM-L6-v2")
    )
    embed_dim_hashing: int = field(default_factory=lambda: _env_int("SCEMA_EMBED_DIM", 512))

    # --- network ---
    hidden_dim: int = field(default_factory=lambda: _env_int("SCEMA_HIDDEN_DIM", 256))
    n_blocks: int = field(default_factory=lambda: _env_int("SCEMA_N_BLOCKS", 3))
    dropout: float = field(default_factory=lambda: _env_float("SCEMA_DROPOUT", 0.1))
    device: str = field(default_factory=lambda: os.getenv("SCEMA_DEVICE", "auto"))

    # --- online training ---
    lr: float = field(default_factory=lambda: _env_float("SCEMA_LR", 3e-4))
    weight_decay: float = field(default_factory=lambda: _env_float("SCEMA_WEIGHT_DECAY", 1e-2))
    batch_size: int = field(default_factory=lambda: _env_int("SCEMA_BATCH_SIZE", 32))
    replay_capacity: int = field(default_factory=lambda: _env_int("SCEMA_REPLAY_CAPACITY", 20_000))
    # Feedback events needed before an automatic training step fires.
    train_every: int = field(default_factory=lambda: _env_int("SCEMA_TRAIN_EVERY", 8))
    steps_per_trigger: int = field(default_factory=lambda: _env_int("SCEMA_STEPS_PER_TRIGGER", 4))
    min_replay_to_train: int = field(default_factory=lambda: _env_int("SCEMA_MIN_REPLAY", 16))

    # --- kernels ---
    use_mojo_kernel: bool = field(default_factory=lambda: _env_bool("SCEMA_USE_MOJO", True))

    # --- server ---
    host: str = field(default_factory=lambda: os.getenv("SCEMA_CORTEX_HOST", "127.0.0.1"))
    port: int = field(default_factory=lambda: _env_int("SCEMA_CORTEX_PORT", 7077))

    def __post_init__(self) -> None:
        self.data_dir = Path(self.data_dir)
        self.data_dir.mkdir(parents=True, exist_ok=True)

    @property
    def checkpoint_path(self) -> Path:
        return self.data_dir / "tastenet.pt"

    @property
    def memory_path(self) -> Path:
        return self.data_dir / "memory.npz"

    @property
    def replay_path(self) -> Path:
        return self.data_dir / "replay.jsonl"

    def resolve_device(self) -> str:
        if self.device != "auto":
            return self.device
        try:
            import torch

            return "cuda" if torch.cuda.is_available() else "cpu"
        except Exception:
            return "cpu"


CONFIG = CortexConfig()
