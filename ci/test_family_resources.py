import json
from pathlib import Path
import tempfile
import unittest
import mkipk


class FamilyLocalization(unittest.TestCase):
    def test_declared_family_brand_is_preserved_in_every_locale(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);(root/'pkg/resources/es').mkdir(parents=True)
            (root/'pkg/appinfo.json').write_text(json.dumps({'title':'Cinema'}))
            (root/'pkg/resources/es/appinfo.json').write_text(json.dumps({'title':'Cinema','appDescription':'Descripción'}))
            app=mkipk.flavor.appinfo_for('family')
            mkipk.stage_resources(root,root/'data',app)
            staged=root/'data/usr/palm/applications'/app['id']/'resources/es/appinfo.json'
            self.assertEqual(json.loads(staged.read_text()),{'title':'PinPin Home','appDescription':'Descripción'})

    def test_existing_suffix_and_stable_localization_bytes_are_unchanged(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);(root/'pkg/resources/en').mkdir(parents=True)
            (root/'pkg/appinfo.json').write_text(json.dumps({'title':'Cinema'}))
            source={'title':'Cinema','appDescription':'Existing description'}
            (root/'pkg/resources/en/appinfo.json').write_text(json.dumps(source))
            for profile,title in [('stable','Cinema'),('debug','Cinema Parents'),('nightly','Cinema Kids')]:
                app=mkipk.flavor.appinfo_for(profile,'20261027')
                mkipk.stage_resources(root,root/'data',app)
                staged=root/'data/usr/palm/applications'/app['id']/'resources/en/appinfo.json'
                self.assertEqual(staged.read_bytes(),(json.dumps({**source,'title':title},ensure_ascii=False,indent=2)+'\n').encode())

    def test_undeclared_brand_change_is_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);(root/'pkg/resources').mkdir(parents=True)
            (root/'pkg/appinfo.json').write_text(json.dumps({'title':'Cinema'}))
            app=mkipk.flavor.appinfo_for('family');app['title']='Undeclared title'
            with self.assertRaises(SystemExit):mkipk.stage_resources(root,root/'data',app)

if __name__=='__main__':unittest.main()
