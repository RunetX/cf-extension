from pathlib import Path
import re
from common import ROOT

def audit():
    cargo = (ROOT / 'Cargo.toml').read_text() + (ROOT / 'crates/model-server/Cargo.toml').read_text()
    if re.search(r'(?m)(?:^\s*\[patch|\bpath\s*=)', cargo):
        raise RuntimeError('External path dependencies / patches are forbidden')
    lock = (ROOT / 'Cargo.lock').read_text()
    if 'git+https://github.com/RunetX/cf-unwrapper?tag=v0.1.0#' not in lock:
        raise RuntimeError('Cargo.lock must pin remote cf-unwrapper v0.1.0')
    for directory in ['crates', 'extension/src', 'extension/test', 'tools', 'docs']:
        for path in (ROOT / directory).rglob('*'):
            if not path.is_file() or '__pycache__' in path.parts:
                continue
            if path.suffix not in {'.rs','.ts','.py','.md','.xml'}:
                continue
            text = path.read_text(encoding='utf-8-sig')
            if re.search(r'[A-Za-z]:[/\\](?:Users|Prj)[/\\]', text):
                raise RuntimeError(f'Machine-specific path: {path.relative_to(ROOT)}')
    forbidden = {'seed','corpus','corpora','platform','node_modules'}
    for path in (ROOT / 'crates').rglob('*'):
        if path.name.lower() in forbidden:
            raise RuntimeError(f'Unexpected resource tree: {path}')
    print('Source audit passed (known patterns only; provenance also requires review).')

if __name__ == '__main__':
    audit()
