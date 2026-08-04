# Alchem-Link v0.2.0

A developer toolkit bridging Alchemy RPC infrastructure and Chainlink oracle patterns into a unified reference layer for Web3 builders.

Install from PyPI:

```bash
pip install alchem-link
```

## What it does

Alchem-Link maps the complementary strengths of Alchemy and Chainlink into actionable developer recipes. It surfaces integration patterns, capability summaries, and step-by-step workflows through both a CLI and a full terminal UI dashboard.

## Terminal UI

Launch the interactive dashboard:

```bash
alchem-link-ui
```

Navigate with arrow keys or j/k. Five panels: Blueprint, Alchemy, Chainlink, Integration, Recipes. All data rendered as formatted cards. Drill into any recipe for a step-by-step checklist. Press q to quit.

Build a standalone exe (no Python required on target machine):

```bash
pip install pyinstaller
pyinstaller alchem-link-ui.spec
# output: dist/alchem-link-ui.exe
```

## CLI

```bash
alchem-link blueprint      # full integration blueprint
alchem-link alchemy        # Alchemy capability summary
alchem-link chainlink      # Chainlink capability summary
alchem-link integration    # cross-system integration map
alchem-link recipes        # all developer recipes
alchem-link recipes <id>   # single recipe by id
alchem-link list           # list available commands
```

## Python API

```python
from alchem_link import (
    build_package_blueprint,
    summarize_alchemy_capabilities,
    summarize_chainlink_capabilities,
    build_integration_map,
    get_recipes,
    get_recipe_by_id,
)
```

## Recipes

| ID | Name |
|---|---|
| `oracle-backed-automation` | Oracle-backed automation |
| `real-time-data-pipeline` | Real-time data pipeline |
| `secure-bridge-experiment` | Secure bridge experiment |
| `ccip-cross-chain-transfer` | CCIP cross-chain transfer |

## Requirements

Python 3.10 or later. The TUI requires a terminal that supports 256 colours (Windows Terminal, iTerm2, or any modern Linux terminal).

## License

MIT
