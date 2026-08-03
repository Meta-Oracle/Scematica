# Alchem Link

This repository is the starting point for an Alchemy x Chainlink developer package focused on cross-referencing, reverse engineering, and packaging the best developer workflows from both ecosystems.

## Vision

Create a first-of-its-kind toolkit that helps builders:
- connect Alchemy-powered blockchain infrastructure with Chainlink-based oracle and automation patterns
- understand how these systems complement each other
- ship faster with reference implementations and practical integration guidance

## Current scaffold

The package currently exposes a small Python blueprint module that can be expanded into documentation, SDK helpers, examples, and deeper analysis modules.

## Quick start

```bash
python -m unittest discover -s tests
```

## CLI usage

Run the package from the repository root with the local source path enabled:

```bash
$env:PYTHONPATH = "src"
python -m alchem_link.cli blueprint
python -m alchem_link.cli alchemy
python -m alchem_link.cli chainlink
python -m alchem_link.cli integration
python -m alchem_link.cli list
```

Use `python -m alchem_link.cli --help` to see the available options.
