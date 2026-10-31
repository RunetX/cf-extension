from common import ROOT, run, npm, server
from audit import audit
import argparse
import shutil
import sys
parser=argparse.ArgumentParser(description='Build a local platform-specific VSIX; never publish')
parser.add_argument('--target', default='win32-x64' if sys.platform == 'win32' else 'linux-x64')
args=parser.parse_args()
audit()
run('cargo','build','--release','--locked','-p','model-server')
binary=server(True)
(ROOT/'extension/bin').mkdir(exist_ok=True)
shutil.copy2(binary, ROOT/'extension/bin'/binary.name)
shutil.copy2(ROOT/'LICENSE', ROOT/'extension/LICENSE')
shutil.copy2(ROOT/'README.md', ROOT/'extension/README.md')
npm('ci','--ignore-scripts')
npm('run','compile')
(ROOT/'extension/dist').mkdir(exist_ok=True)
npm('exec','--','vsce','package','--no-dependencies','--target',args.target,'--out',f'dist/cf-extension-{args.target}.vsix')
