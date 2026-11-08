"""Package the local skill and source; optionally update a named existing skill directory.
No output/cache/source of analyzed repositories is included.
"""
import argparse
import shutil
import zipfile
from pathlib import Path

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument('--install-to', type=Path)
args = parser.parse_args()
out = root / 'output'
out.mkdir(exist_ok=True)
files = ['SKILL.md', 'README.md', 'PRODUCT_PLAN.md', 'VERIFICATION.md',
         'USER_TEST_PROTOCOL.md', 'LICENSE', 'Cargo.toml', 'Cargo.lock']
folders = ['.cargo', 'agents', 'assets', 'bin', 'src', 'references', 'scripts', 'tests', 'examples']
skill_files = [root / name for name in files if (root / name).is_file()]
skill_files += [p for name in folders for p in (root / name).rglob('*') if p.is_file()]
skill_zip = out / 'code-trail-0.4.0-skill.zip'
with zipfile.ZipFile(skill_zip, 'w', zipfile.ZIP_DEFLATED) as archive:
    for file in skill_files:
        archive.write(file, Path('code-trail') / file.relative_to(root))
if args.install_to:
    destination = args.install_to.resolve()
    if destination.name != 'code-trail':
        raise SystemExit('The installation target must be the named code-trail skill directory.')
    destination.mkdir(parents=True, exist_ok=True)
    for file in skill_files:
        target = destination / file.relative_to(root)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(file, target)
source_files = skill_files + [p for p in (root / 'extension').rglob('*') if p.is_file()
    and not any(part in {'node_modules', 'out', 'test-output', '.vscode-test'} for part in p.relative_to(root / 'extension').parts)
    and p.suffix != '.vsix']
source_files += [p for p in (root / '.github').rglob('*') if p.is_file()]
source_files += [root / '.gitignore']
with zipfile.ZipFile(out / 'code-trail-0.4.0-source.zip', 'w', zipfile.ZIP_DEFLATED) as archive:
    for file in source_files:
        archive.write(file, Path('code-trail') / file.relative_to(root))
print(skill_zip)
print(out / 'code-trail-0.4.0-source.zip')
