"""TasteNet -- the learned judgement at the centre of Scematica Omni-Agent.

Why a network at all, rather than prompting Grok to "rate this 1-10"?

* It is cheap, so the sense loop can score hundreds of candidates per cycle
  instead of a handful.
* It is personal: it learns this operator's taste from real approve/reject
  decisions, which no amount of prompt text encodes.
* It is falsifiable: heads are trained against outcomes that actually
  happened, so improvement is measurable rather than asserted.

Three heads share one trunk, because the tasks are correlated and the labels
arrive sparsely from different sources:

    salience   BCE   "is this worth engaging with at all?"
                     labels: operator decisions + reflection outcomes
    taste      BCE   "will the operator approve this specific draft?"
                     labels: approve(1) / reject(0) from the Telegram cockpit
    resonance  MSE   "how much engagement will this earn?" (log1p scale)
                     labels: real post metrics, fed back after the fact

Multi-task sharing is what makes sparse feedback usable: an approve/reject on
one draft still improves the trunk that salience and resonance read from.
"""
from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Dict, Optional

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F

from .config import CONFIG, CortexConfig

log = logging.getLogger("scema.model")

# Hand-built context features appended to the text embedding. Keeping these
# explicit (and ordered) means a checkpoint stays interpretable.
FEATURE_NAMES: tuple[str, ...] = (
    "log_followers",      # reach of the author we might reply to
    "log_likes",          # traction the source post already has
    "log_reposts",
    "log_replies",
    "age_hours",          # fresh things matter more
    "is_reply",
    "is_mention_of_us",
    "author_is_known",    # have we interacted before
    "text_len_norm",
    "has_link",
    "has_media",
    "question_mark",      # invitations to respond
    "sentiment",          # -1..1, cheap lexicon estimate
    "novelty",            # 1 - max cosine vs recent memory
    "topic_affinity",     # cosine vs the agent's stated interests
    "hour_sin",           # crude circadian context
    "hour_cos",
)
N_FEATURES = len(FEATURE_NAMES)

HEADS: tuple[str, ...] = ("salience", "taste", "resonance")


def features_to_vector(feats: Optional[Dict[str, float]]) -> np.ndarray:
    """Project a (possibly partial) feature dict into the fixed slot order."""
    vec = np.zeros(N_FEATURES, dtype=np.float32)
    if not feats:
        return vec
    for i, name in enumerate(FEATURE_NAMES):
        val = feats.get(name)
        if val is None:
            continue
        try:
            fval = float(val)
        except (TypeError, ValueError):
            continue
        if not np.isfinite(fval):
            continue
        vec[i] = fval
    return vec


class FeatureNorm(nn.Module):
    """Standardise context features with running statistics and a variance floor.

    Replaces nn.BatchNorm1d, which was actively harmful here. Two failures it
    fixes, both measured rather than theorised (see docs/DECISIONS.md):

    1. **Amplification of near-constant features.** BatchNorm divides by the
       batch standard deviation. A feature that barely varies across an online
       batch -- which most of these do -- gets its noise multiplied into a
       dominant signal. The net then learns a shortcut: in testing it latched
       onto `text_len_norm` and scored by post length instead of meaning,
       dropping held-out separation from 1.00 to 0.00. The variance floor
       means a feature with no real spread contributes ~0 instead.

    2. **Train/eval divergence.** BatchNorm normalises by batch statistics
       while training and running statistics while scoring, so a model that
       fit perfectly could still score incoherently. This always uses running
       statistics; training only updates them.
    """

    def __init__(self, n_features: int, momentum: float = 0.02, var_floor: float = 0.01) -> None:
        super().__init__()
        self.momentum = momentum
        self.var_floor = var_floor
        self.register_buffer("running_mean", torch.zeros(n_features))
        self.register_buffer("running_var", torch.ones(n_features))
        self.weight = nn.Parameter(torch.ones(n_features))
        self.bias = nn.Parameter(torch.zeros(n_features))

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        if self.training and x.shape[0] > 1:
            with torch.no_grad():
                batch_mean = x.mean(dim=0)
                batch_var = x.var(dim=0, unbiased=False)
                self.running_mean.mul_(1 - self.momentum).add_(self.momentum * batch_mean)
                self.running_var.mul_(1 - self.momentum).add_(self.momentum * batch_var)
        var = self.running_var.clamp(min=self.var_floor)
        normed = (x - self.running_mean) / var.sqrt()
        # Clamp the tail: one absurd feature value from the TS side should not
        # be able to saturate the trunk.
        return self.weight * normed.clamp(-5.0, 5.0) + self.bias


class ResidualBlock(nn.Module):
    """Pre-norm residual MLP block: stable under the tiny, noisy online
    batches this model trains on."""

    def __init__(self, dim: int, dropout: float) -> None:
        super().__init__()
        self.norm = nn.LayerNorm(dim)
        self.fc1 = nn.Linear(dim, dim * 2)
        self.fc2 = nn.Linear(dim * 2, dim)
        self.drop = nn.Dropout(dropout)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        h = self.norm(x)
        h = F.gelu(self.fc1(h))
        h = self.drop(self.fc2(h))
        return x + h


class TasteNet(nn.Module):
    def __init__(
        self,
        embed_dim: int,
        hidden_dim: int = 256,
        n_blocks: int = 3,
        dropout: float = 0.1,
        n_features: int = N_FEATURES,
        feature_dropout: float = 0.25,
    ) -> None:
        super().__init__()
        self.embed_dim = embed_dim
        self.n_features = n_features

        # Features arrive unnormalised from the TS side and sit on wildly
        # different scales; running stats keep one loud feature from
        # dominating the trunk early in training.
        self.feat_norm = FeatureNorm(n_features)
        # Probability of blanking the entire feature block for a row during
        # training. Without it the net can satisfy the loss using context
        # features alone and never learn to read the text -- exactly the
        # shortcut that broke generalisation before. Forcing the trunk to
        # survive without features makes the embedding carry the signal.
        self.feature_dropout = feature_dropout
        self.stem = nn.Linear(embed_dim + n_features, hidden_dim)
        self.blocks = nn.ModuleList(
            [ResidualBlock(hidden_dim, dropout) for _ in range(n_blocks)]
        )
        self.trunk_norm = nn.LayerNorm(hidden_dim)
        self.heads = nn.ModuleDict(
            {
                name: nn.Sequential(
                    nn.Linear(hidden_dim, hidden_dim // 2),
                    nn.GELU(),
                    nn.Linear(hidden_dim // 2, 1),
                )
                for name in HEADS
            }
        )

    def _prepare_feats(self, feats: torch.Tensor) -> torch.Tensor:
        normed = self.feat_norm(feats)
        if self.training and self.feature_dropout > 0:
            # Blank whole rows, not individual features: the point is to make
            # some fraction of every batch text-only.
            keep = (
                torch.rand(normed.shape[0], 1, device=normed.device) >= self.feature_dropout
            ).float()
            normed = normed * keep
        return normed

    def trunk(self, emb: torch.Tensor, feats: torch.Tensor) -> torch.Tensor:
        h = self.stem(torch.cat([emb, self._prepare_feats(feats)], dim=-1))
        for block in self.blocks:
            h = block(h)
        return self.trunk_norm(h)

    def forward(self, emb: torch.Tensor, feats: torch.Tensor) -> Dict[str, torch.Tensor]:
        """Raw logits per head (no activation) for numerically stable losses."""
        h = self.trunk(emb, feats)
        return {name: head(h).squeeze(-1) for name, head in self.heads.items()}

    @torch.no_grad()
    def score(self, emb: torch.Tensor, feats: torch.Tensor) -> Dict[str, torch.Tensor]:
        """Inference view: probabilities for classifier heads, engagement
        counts for resonance."""
        self.eval()
        logits = self.forward(emb, feats)
        return {
            "salience": torch.sigmoid(logits["salience"]),
            "taste": torch.sigmoid(logits["taste"]),
            # resonance trains on log1p(engagement); invert for a readable number
            "resonance": torch.expm1(F.softplus(logits["resonance"])),
        }


@dataclass
class LossWeights:
    salience: float = 1.0
    taste: float = 1.0
    resonance: float = 0.5


def masked_losses(
    logits: Dict[str, torch.Tensor],
    targets: Dict[str, torch.Tensor],
    masks: Dict[str, torch.Tensor],
    weights: LossWeights = LossWeights(),
) -> tuple[torch.Tensor, Dict[str, float]]:
    """Multi-task loss over only the labels that actually exist.

    Feedback is sparse and heterogeneous -- an approve/reject carries a taste
    label but no engagement number yet, and a metrics update carries resonance
    with no fresh taste label. Masking per head lets one replay buffer hold
    both without inventing labels.
    """
    total = logits["salience"].new_zeros(())
    parts: Dict[str, float] = {}
    any_label = False

    for name in HEADS:
        mask = masks[name]
        if mask.sum() == 0:
            continue
        pred = logits[name][mask]
        tgt = targets[name][mask]
        if name == "resonance":
            loss = F.mse_loss(F.softplus(pred), torch.log1p(tgt.clamp(min=0.0)))
        else:
            loss = F.binary_cross_entropy_with_logits(pred, tgt)
        total = total + getattr(weights, name) * loss
        parts[name] = float(loss.detach())
        any_label = True

    if not any_label:
        parts["skipped"] = 1.0
    return total, parts


def build_model(embed_dim: int, cfg: CortexConfig | None = None) -> TasteNet:
    cfg = cfg or CONFIG
    model = TasteNet(
        embed_dim=embed_dim,
        hidden_dim=cfg.hidden_dim,
        n_blocks=cfg.n_blocks,
        dropout=cfg.dropout,
    )
    log.info("TasteNet built: embed_dim=%d params=%d", embed_dim, count_params(model))
    return model


def count_params(model: nn.Module) -> int:
    return sum(p.numel() for p in model.parameters())
