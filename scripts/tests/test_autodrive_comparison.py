import copy
import importlib.util
from pathlib import Path
import unittest

spec=importlib.util.spec_from_file_location('compare_autodrive',Path(__file__).parents[1]/'compare-autodrive.py')
module=importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ComparisonTests(unittest.TestCase):
    def report(self):
        return dict(engine='Factorio',suite='long',seed=12345,results=[
            dict(name='success',initial=dict(x=1),status='arrived',total_ticks=100,path_length=50,damage=0),
            dict(name='failure',initial=dict(x=2),status='arrived',total_ticks=200,path_length=100,damage=0)])

    def test_failures_do_not_appear_as_speed_improvements(self):
        old=self.report()
        new=copy.deepcopy(old)
        new['results'][0].update(total_ticks=90,path_length=40)
        new['results'][1].update(status='stalled',total_ticks=1,path_length=0,damage=5)
        result=module.compare(old,new)
        self.assertEqual(result['common_successes'],1)
        self.assertAlmostEqual(result['total_time_change_pct'],-10)
        self.assertAlmostEqual(result['total_path_change_pct'],-20)
        self.assertEqual(result['candidate']['damage'],5)
        self.assertEqual(result['candidate']['arrived'],1)
        self.assertIsNone(result['pairs'][1]['time_change_pct'])

    def test_mismatched_initial_states_are_rejected(self):
        old=self.report()
        new=copy.deepcopy(old)
        new['results'][0]['initial']['x']=3
        with self.assertRaisesRegex(ValueError,'different initial state'):
            module.compare(old,new)
