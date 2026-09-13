"""HTTP surface for the cortex, consumed by the TypeScript agent.

    python -m scema_cortex.server

Bound to 127.0.0.1 by default and deliberately unauthenticated: this is a
local sidecar, not a public service. If you ever bind it to 0.0.0.0, put an
auth layer in front -- /feedback writes to the agent's learned judgement, and
an open one is a way to teach it anything you like.
"""
from __future__ import annotations

import asyncio
import logging
import os
from contextlib import asynccontextmanager
from typing import Any, Dict, List, Optional

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field

from .config import CONFIG
from .service import CortexService, get_service

logging.basicConfig(
    level=os.getenv("SCEMA_LOG_LEVEL", "INFO"),
    format="%(asctime)s  %(name)-14s %(levelname)-7s %(message)s",
    datefmt="%H:%M:%S",
)
log = logging.getLogger("scema.server")

AUTOSAVE_SECONDS = 120


# ----------------------------------------------------------------- schemas


class ScoreItem(BaseModel):
    id: Optional[str] = None
    text: str
    features: Dict[str, float] = Field(default_factory=dict)


class ScoreRequest(BaseModel):
    items: List[ScoreItem]
    weights: Optional[Dict[str, float]] = None


class FeedbackRequest(BaseModel):
    text: str
    salience: Optional[float] = None
    taste: Optional[float] = None
    resonance: Optional[float] = None
    features: Dict[str, float] = Field(default_factory=dict)
    source: str = "unknown"
    ref: Optional[str] = None
    autotrain: bool = True


class RememberRequest(BaseModel):
    text: str
    surface: str = "system"
    kind: str = "observation"
    meta: Dict[str, Any] = Field(default_factory=dict)


class RecallRequest(BaseModel):
    query: str
    k: int = 6
    surfaces: Optional[List[str]] = None
    kinds: Optional[List[str]] = None
    half_life_hours: float = 72.0


class EmbedRequest(BaseModel):
    texts: List[str]


class TrainRequest(BaseModel):
    steps: int = 8


# ------------------------------------------------------------------- app


@asynccontextmanager
async def lifespan(app: FastAPI):
    service = get_service()
    app.state.service = service

    async def autosave() -> None:
        # Periodic, not per-write: checkpointing on every feedback event would
        # dominate the cost of the training step it follows.
        while True:
            try:
                await asyncio.sleep(AUTOSAVE_SECONDS)
                result = service.save()
                if result.get("saved"):
                    log.info("autosave: checkpoint + memory written")
            except asyncio.CancelledError:
                raise
            except Exception as exc:
                log.warning("autosave failed (%s)", exc)

    task = asyncio.create_task(autosave())
    try:
        yield
    finally:
        task.cancel()
        # Shutdown is the one save that must not be skipped.
        try:
            service.save(force=True)
            log.info("shutdown: state persisted")
        except Exception as exc:
            log.error("shutdown save failed (%s)", exc)


app = FastAPI(
    title="Scema Cortex",
    version="0.1.0",
    description="Learned salience, taste and resonance for Scematica Omni-Agent.",
    lifespan=lifespan,
)


def _service() -> CortexService:
    return app.state.service


@app.get("/health")
async def health() -> Dict[str, Any]:
    svc = _service()
    return {
        "ok": True,
        "embedder": svc.embedder.name,
        "dim": svc.embedder.dim,
        "device": svc.device,
        "memories": len(svc.memory),
        "train_steps": svc.trainer.total_steps,
    }


@app.get("/stats")
async def stats() -> Dict[str, Any]:
    return _service().stats()


@app.post("/embed")
async def embed(req: EmbedRequest) -> Dict[str, Any]:
    vectors = _service().embed(req.texts)
    return {"dim": int(vectors.shape[1]) if vectors.size else 0, "vectors": vectors.tolist()}


@app.post("/score")
async def score(req: ScoreRequest) -> Dict[str, Any]:
    if not req.items:
        return {"scored": []}
    if len(req.items) > 512:
        raise HTTPException(status_code=413, detail="score at most 512 items per call")
    items = [item.model_dump() for item in req.items]
    return {"scored": _service().score(items, req.weights)}


@app.post("/feedback")
async def feedback(req: FeedbackRequest) -> Dict[str, Any]:
    if req.salience is None and req.taste is None and req.resonance is None:
        raise HTTPException(status_code=422, detail="at least one head label is required")
    return _service().feedback(
        text=req.text,
        salience=req.salience,
        taste=req.taste,
        resonance=req.resonance,
        features=req.features,
        source=req.source,
        ref=req.ref,
        autotrain=req.autotrain,
    )


@app.post("/train")
async def train(req: TrainRequest) -> Dict[str, Any]:
    return _service().train(req.steps)


@app.post("/remember")
async def remember(req: RememberRequest) -> Dict[str, Any]:
    return _service().remember(req.text, surface=req.surface, kind=req.kind, meta=req.meta)


@app.post("/recall")
async def recall(req: RecallRequest) -> Dict[str, Any]:
    hits = _service().recall(
        req.query,
        k=req.k,
        surfaces=req.surfaces,
        kinds=req.kinds,
        half_life_hours=req.half_life_hours,
    )
    return {"hits": hits}


@app.post("/save")
async def save() -> Dict[str, Any]:
    return _service().save(force=True)


def main() -> None:
    import uvicorn

    log.info("starting Scema Cortex on http://%s:%d", CONFIG.host, CONFIG.port)
    uvicorn.run(app, host=CONFIG.host, port=CONFIG.port, log_level="warning")


if __name__ == "__main__":
    main()
