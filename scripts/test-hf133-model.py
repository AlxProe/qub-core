import json
from pathlib import Path
import random
import statistics

import hf133_daa_reference as ref


def simulate(algorithm, half_life, seed, scenario, rounds=4000):
    rng=random.Random(seed)
    initial_bits=ref.compact(2**256//(10**9*48)-1)
    headers=[{'bits':initial_bits,'time':1_000_000+i*60} for i in range(21)]
    now=headers[-1]['time']+60.; output=[]
    for i in range(rounds):
        rate=10**9
        if scenario=='step': rate=rate*(10 if 1000<=i<2000 else .1 if 2000<=i<3000 else 1)
        if scenario=='switch':rate=rate*(10 if i%200<100 else 1)
        bits=ref.v2(headers) if algorithm=='v2' else ref.anchored(headers,20,half_life)
        mean=2**256/(ref.target(bits)+1)/rate
        duration=(rng.expovariate(1/mean) if seed is not None else mean)+12
        headers.append({'bits':bits,'time':max(int(now),ref.mtp(headers)+1)})
        now+=duration
        output.append(duration)
    tail=output[1000:]
    return {'seed':seed,'scenario':scenario,'algorithm':algorithm,'half_life':half_life,
            'late_mean_seconds':statistics.mean(output[3500:]),
            'tail_p99_seconds':sorted(tail)[int(len(tail)*.99)],
            'max_seconds':max(tail),'over20min':sum(x>1200 for x in tail),
            'segment_mean_seconds':[statistics.mean(output[j:j+500]) for j in range(0,rounds,500)]}


rows=[]
for scenario in ['steady','step','switch']:
    for algorithm,half in [('v2',0),('asert',1800),('asert',3600),('asert',7200),('asert',17280)]:
        samples=[simulate(algorithm,half,seed,scenario) for seed in [None,*range(8)]]
        rows.extend(samples)
        random_rows=samples[1:]
        print(scenario,algorithm,half,'late mean',round(statistics.mean(r['late_mean_seconds'] for r in random_rows),2),
              'max',round(max(r['max_seconds'] for r in random_rows)), 'p99 mean',round(statistics.mean(r['tail_p99_seconds'] for r in random_rows)),
              'over20m',sum(r['over20min'] for r in random_rows),'deterministic max',round(samples[0]['max_seconds'],2),flush=True)
# These thresholds are model-specific regression gates, not live guarantees.
steady=[r for r in rows if r['scenario']=='steady' and r['algorithm']=='asert' and r['half_life']==3600]
step=[r for r in rows if r['scenario']=='step' and r['algorithm']=='asert' and r['half_life']==3600]
assert all(54 < r['late_mean_seconds'] < 66 and r['max_seconds'] < 600 for r in steady)
assert all(54 < r['late_mean_seconds'] < 66 for r in step)
assert any(r['max_seconds'] > 1200 for r in rows if r['algorithm']=='v2' and r['scenario']=='steady')
print('HF133 MODEL: PASS - 135 scenarios, 540000 rounds; synthetic assumptions, not mainnet prediction')
