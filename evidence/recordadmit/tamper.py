"""Run the named test with the admission gate deliberately broken; restore source."""
from pathlib import Path
import subprocess
root = Path(__file__).resolve().parents[2]
source = root / 'rust/crates/safemesh-crdt/src/lib.rs'
evidence = Path(__file__).resolve().parent
original = source.read_text()
needle = '        if let Some(&index) = self.seen.get(&record.id) {\n'
assert original.count(needle) == 1
try:
    source.write_text(original.replace(needle, needle + '            apply(&record.delta); // TAMPER: apply rejected payload\n'))
    with (evidence / 'tamper.log').open('w') as output:
        result = subprocess.run(['cargo', 'test', '--manifest-path', str(root / 'rust/Cargo.toml'), '-p', 'safemesh-crdt', '--test', 'record_admission', 'record_1_1_live_and_replay', '--', '--nocapture'], stdout=output, stderr=subprocess.STDOUT)
    (evidence / 'tamper.exit').write_text(str(result.returncode) + '\n')
finally:
    source.write_text(original)
code = (evidence / 'tamper.exit').read_text().strip()
print('tamper cargo test exit (read from file):', code)
print((evidence / 'tamper.log').read_text())
assert code == '101'
assert 'live 9; replay 5' in (evidence / 'tamper.log').read_text()
