# alchem-link-ui.spec
# Build: pyinstaller alchem-link-ui.spec
from PyInstaller.utils.hooks import collect_data_files, collect_submodules

datas = collect_data_files("textual")
hiddenimports = collect_submodules("textual") + collect_submodules("alchem_link")

a = Analysis(
    ["src/alchem_link/tui.py"],
    pathex=["src"],
    binaries=[],
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    runtime_hooks=[],
    excludes=[],
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
    console=True,   # must be True — Textual needs a real terminal
    icon=None,
)
