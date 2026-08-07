# alchem-link.spec — the CLI as a single-file binary.
#
#   pyinstaller alchem-link.spec
#
# There is nothing to collect. The package has no runtime dependencies, and as of 0.23.0
# that includes the user interface: `alchem_link.term` is the terminal system, so a build
# of this spec bundles the standard library and this package and nothing else.
#
# `console=True` is not optional. A windowed build has no terminal to write to, and this
# program's entire output is terminal output.
from PyInstaller.utils.hooks import collect_submodules

# Collected explicitly because several modules are imported lazily inside functions —
# `alchem_link.dashboard` from `cli._cmd_ui`, `alchem_link.shell` from `cli._cmd_shell`,
# `alchem_link.agent` from the chat path — and PyInstaller's static analysis does not
# follow a function-scoped import.
hiddenimports = collect_submodules("alchem_link")

a = Analysis(
    ["src/alchem_link/cli.py"],
    pathex=["src"],
    binaries=[],
    datas=[],
    hiddenimports=hiddenimports,
    hookspath=[],
    runtime_hooks=[],
    # Nothing here is used, and each pulls in tens of megabytes.
    excludes=["tkinter", "numpy", "pandas", "matplotlib", "PIL", "test", "unittest"],
    noarchive=False,
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    name="alchem-link",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    console=True,
    icon=None,
)
