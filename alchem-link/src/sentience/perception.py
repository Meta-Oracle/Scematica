"""§2 Data/Perception  D = A_aud × Vis × X × I"""
from __future__ import annotations
from dataclasses import dataclass
from .types import Bounded


@dataclass
class Perception:
    audio: Bounded      # A_aud
    visual: Bounded     # Vis
    sensory: Bounded    # X
    integrity: Bounded  # I

    def __init__(self, audio=1.0, visual=1.0, sensory=1.0, integrity=1.0):
        self.audio = Bounded(audio)
        self.visual = Bounded(visual)
        self.sensory = Bounded(sensory)
        self.integrity = Bounded(integrity)

    def data_ratio(self) -> Bounded:
        """D = A_aud × Vis × X × I"""
        return Bounded(
            self.audio.value * self.visual.value *
            self.sensory.value * self.integrity.value
        )

    @classmethod
    def default(cls) -> "Perception":
        return cls()
