"""Inspect a local VSIX and exercise its bundled server and real Extension Host."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile
from common import ROOT, HIDDEN, npm


def inspect_vsix(path):
    with zipfile.ZipFile(path) as archive:
        names = set(archive.namelist())
        binary = 'model-server.exe' if os.name == 'nt' else 'model-server'
        required = {'extension/package.json', 'extension/out/src/extension.js',
                    'extension/out/src/client.js', 'extension/out/src/navigator.js',
                    'extension/syntaxes/bsl.tmLanguage.json',
                    'extension/media/constructor.svg', 'extension/media/logo-k.png', 'extension/bin/' + binary,
                    'extension/LICENSE.txt', 'extension/readme.md'}
        missing = required - names
        if missing:
            raise RuntimeError(f'Incomplete VSIX: {sorted(missing)}')
        forbidden = {'node_modules', 'test', 'tests', 'src', 'corpus', 'corpora', 'seed', 'platform'}
        for name in names:
            parts = Path(name).parts
            # Compiled out/src is runtime code; source, tests and dependency trees are excluded.
            if any(p in forbidden for p in parts[1:] if not (p == 'src' and 'out' in parts)):
                raise RuntimeError(f'Development/resource asset in VSIX: {name}')
        manifest = json.loads(archive.read('extension/package.json'))
        if manifest.get('icon') != 'media/logo-k.png':
            raise RuntimeError('The extension must use the Constructor brand logo')
        if not archive.read('extension/media/logo-k.png').startswith(b'\x89PNG\r\n\x1a\n'):
            raise RuntimeError('Invalid brand PNG')
        if manifest.get('main') != './out/src/extension.js':
            raise RuntimeError('Unexpected VSIX entry point')
        if len(archive.read('extension/bin/' + binary)) < 10000:
            raise RuntimeError('Bundled server is not a runtime binary')
        print(f'VSIX verified: {path.name}, {len(names)} entries, {path.stat().st_size} bytes', flush=True)


def smoke(path):
    inspect_vsix(path)
    with tempfile.TemporaryDirectory(prefix='cf-extension-package-') as temp:
        extraction = Path(temp)
        with zipfile.ZipFile(path) as archive:
            archive.extractall(extraction)
        extension = extraction / 'extension'
        binary = extension / 'bin' / ('model-server.exe' if os.name == 'nt' else 'model-server')
        if os.name != 'nt':
            binary.chmod(0o755)
        env = dict(os.environ, CF_EXTENSION_PACKAGE_DIR=str(extension), CF_EXTENSION_SERVER=str(binary))
        command = [shutil.which('npm.cmd' if os.name == 'nt' else 'npm') or 'npm', 'run', 'test:host']
        result = subprocess.run(command, cwd=ROOT / 'extension', env=env, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, text=True, encoding='utf-8', errors='replace', **HIDDEN)
        print('\n'.join(result.stdout.splitlines()[-30:]), flush=True)
        result.check_returncode()
        print('Packaged Extension Host passed with bundled default server path (no serverPath override).', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('vsix', type=Path)
    parser.add_argument('--inspect-only', action='store_true')
    args = parser.parse_args()
    (inspect_vsix if args.inspect_only else smoke)(args.vsix.resolve())
