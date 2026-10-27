import pathlib
import shutil
import subprocess
import unittest


class AdministrativeReceipt(unittest.TestCase):
    @unittest.skipUnless(shutil.which('node'), 'administrative Node tool is unavailable')
    def test_precise_identity_and_writer_policy(self):
        root = pathlib.Path(__file__).resolve().parents[1]
        subprocess.run(['node', str(root/'tools/test_local_cache_receipt.cjs')],
                       cwd=root, check=True, timeout=15)


if __name__ == '__main__':
    unittest.main()
