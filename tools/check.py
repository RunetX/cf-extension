from common import run, npm
from audit import audit

audit()
run('cargo','fmt','--all','--','--check')
run('cargo','check','--workspace','--locked')
run('cargo','test','--workspace','--locked')
npm('ci','--ignore-scripts')
npm('test')
