from common import ROOT, run, npm, server
from audit import audit
import argparse
import shutil
import sys
import platform
from verify_package import inspect_vsix
parser=argparse.ArgumentParser(description='Build a local platform-specific VSIX; never publish')
parser.add_argument('--target', default='win32-x64' if sys.platform == 'win32' else 'linux-x64')
args=parser.parse_args()
native_platform = 'win32' if sys.platform == 'win32' else 'linux' if sys.platform.startswith('linux') else 'unsupported'
native_arch = 'arm64' if platform.machine().lower() in {'arm64', 'aarch64'} else 'x64' if platform.machine().lower() in {'amd64', 'x86_64'} else 'unsupported'
if args.target != f'{native_platform}-{native_arch}':
    parser.error('VSIX target must match the native release binary; cross-compilation is not performed')
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

inspect_vsix(ROOT/'extension/dist'/f'cf-extension-{args.target}.vsix')
