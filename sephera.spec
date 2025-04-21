# -*- mode: python ; coding: utf-8 -*-
import os

block_cipher = None
binary_name = os.getenv('BINARY_NAME', 'sephera')

a = Analysis(
    ['main.py'],
    pathex=['.', 'etc', 'etc/generate'],
    binaries=[],
    datas=[
        ('etc/*', 'etc'),
        ('etc/generate/*', 'etc/generate'),
        ('config/*', 'config')
    ],
    hiddenimports=[
        'etc.subclass',
        'etc.generate.config_data',
        'etc.generate.config_help'
    ],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
    optimize=0,
)
pyz = PYZ(a.pure, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name=binary_name,
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)