from common import ROOT, run, server
import argparse
parser = argparse.ArgumentParser(description='Generate our own synthetic configuration with library APIs')
parser.add_argument('destination')
args=parser.parse_args()
run('cargo','build','--locked','-p','model-server')
run(server(),'--generate-fixture',args.destination)
