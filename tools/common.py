import argparse
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
HIDDEN = {'creationflags': subprocess.CREATE_NO_WINDOW} if sys.platform == 'win32' else {}

def run(*args, cwd=ROOT):
    print('+', ' '.join(map(str, args)), flush=True)
    result = subprocess.run(list(map(str, args)), cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding='utf-8', errors='replace', **HIDDEN)
    print('\n'.join(result.stdout.splitlines()[-50:]), flush=True)
    result.check_returncode()

def npm(*args):
    run(shutil.which('npm.cmd' if sys.platform == 'win32' else 'npm') or 'npm', *args, cwd=ROOT / 'extension')

def server(release=False):
    return ROOT / 'target' / ('release' if release else 'debug') / ('model-server.exe' if sys.platform == 'win32' else 'model-server')
