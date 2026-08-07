# alchem-link-ui.spec — the full-screen dashboard as a single-file binary.
#
#   pyinstaller alchem-link-ui.spec
#
# This build used to bundle Textual and everything it collects. It no longer bundles
# anything: `alchem_link.term` is the terminal system, so the binary is the standard
# library plus this package.
#
# One behaviour is specific to the frozen build and lives in `alchem_link.term.boot`. A
# binary launched by double-click lands in a brand-new console with default colours and
# no `TERM` at all, which is exactly the case where colour detection has the fewest hints
# and where theming matters most. `boot.initialize` recognises the frozen case, enables
# Windows VT processing, and repaints the terminal's own default background, foreground
# and cursor via OSC 11/10/12 — so the binary looks like the product from its first
# frame, and hands the terminal back on exit.
from PyInstaller.utils.hooks import collect_submodules

hiddenimports = collect_submodules("alchem_link")

a = Analysis(
    ["src/alchem_link/dashboard.py"],
    pathex=["src"],
    binaries=[],
    datas=[],
    hiddenimports=hiddenimports,
    hookspath=[],
    runtime_hooks=[],
    excludes=["tkinter", "numpy", "pandas", "matplotlib", "PIL", "test", "unittest"],
    noarchive=False,
)

pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    name="alchem-link-ui",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    console=True,   # must be True — the dashboard needs a real terminal
    icon=None,
)
