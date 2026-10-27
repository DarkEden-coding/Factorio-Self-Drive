# Fine-tune results

Holdout maps, 200 episodes per scenario, seed 12345. Collision columns are contact ticks per episode.

- reactive-1: first model, `python/runs/reactive-1/best/model.pt`
- ft-strong: fine-tuned from reactive-1 on the scenario mix, collision -0.5, nest_zone -0.3, damage -10, 20M steps (RTX 5060). `python/runs/ft-strong/best/model.pt`
- ft-xstrong: fine-tuned from ft-strong iteration 350, collision -1.0, damage -15, 12M steps (M5). `python/runs/ft-xstrong/best/model.pt`
- A*: distance-field A* + pure pursuit baseline

## standard

| policy | success | SPL | tree | rock | cliff | water | tree-hit eps | zone time | reversed eps | damage | time s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| reactive-1 | 0.985 | 0.949 | 2.65 | 11.23 | 4.38 | 5.64 | 0.150 | 0.000 | 0.130 | 0.29 | 3.55 |
| ft-strong | 0.995 | 0.972 | 1.45 | 0.27 | 0.00 | 9.21 | 0.110 | 0.000 | 0.260 | 0.02 | 3.58 |
| ft-xstrong | 1.000 | 0.987 | 0.82 | 0.30 | 0.00 | 0.00 | 0.105 | 0.000 | 0.335 | 0.01 | 3.53 |
| A* | 0.945 | 0.929 | 0.05 | 0.00 | 0.00 | 0.00 | 0.040 | 0.000 | 0.180 | 0.00 | 4.68 |

## long

| policy | success | SPL | tree | rock | cliff | water | tree-hit eps | zone time | reversed eps | damage | time s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| reactive-1 | 0.805 | 0.799 | 8.82 | 61.03 | 113.34 | 89.74 | 0.615 | 0.001 | 0.035 | 8.26 | 20.46 |
| ft-strong | 0.960 | 0.955 | 8.40 | 0.73 | 47.28 | 0.03 | 0.390 | 0.000 | 0.205 | 4.51 | 20.28 |
| ft-xstrong | 0.970 | 0.965 | 2.35 | 11.67 | 26.78 | 5.91 | 0.390 | 0.001 | 0.235 | 2.07 | 20.35 |
| A* | 0.940 | 0.937 | 5.71 | 0.02 | 0.01 | 0.00 | 0.270 | 0.000 | 0.240 | 0.08 | 28.86 |

## forest

| policy | success | SPL | tree | rock | cliff | water | tree-hit eps | zone time | reversed eps | damage | time s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| reactive-1 | 0.995 | 0.956 | 24.55 | 0.01 | 0.00 | 0.00 | 0.775 | 0.000 | 0.245 | 0.20 | 2.71 |
| ft-strong | 1.000 | 0.973 | 5.71 | 0.00 | 0.00 | 0.00 | 0.645 | 0.000 | 0.395 | 0.07 | 2.51 |
| ft-xstrong | 0.990 | 0.967 | 9.21 | 0.00 | 0.00 | 0.00 | 0.605 | 0.000 | 0.420 | 0.08 | 2.51 |
| A* | 0.940 | 0.869 | 19.37 | 7.47 | 0.00 | 0.00 | 0.535 | 0.001 | 0.430 | 0.13 | 4.04 |

## reverse

| policy | success | SPL | tree | rock | cliff | water | tree-hit eps | zone time | reversed eps | damage | time s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| reactive-1 | 0.915 | 0.796 | 7.70 | 27.02 | 30.09 | 28.78 | 0.240 | 0.007 | 0.680 | 0.42 | 3.60 |
| ft-strong | 0.985 | 0.940 | 3.52 | 3.85 | 7.86 | 5.38 | 0.155 | 0.012 | 0.965 | 0.08 | 3.41 |
| ft-xstrong | 0.985 | 0.939 | 1.86 | 0.21 | 5.18 | 9.68 | 0.145 | 0.013 | 0.965 | 0.05 | 3.32 |
| A* | 0.935 | 0.841 | 15.73 | 4.77 | 6.32 | 13.90 | 0.230 | 0.013 | 0.475 | 0.09 | 5.04 |

## nest

| policy | success | SPL | tree | rock | cliff | water | tree-hit eps | zone time | reversed eps | damage | time s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| reactive-1 | 0.970 | 0.957 | 2.89 | 17.36 | 23.55 | 0.00 | 0.365 | 0.012 | 0.085 | 2.51 | 7.55 |
| ft-strong | 1.000 | 0.990 | 1.80 | 1.26 | 0.17 | 0.00 | 0.260 | 0.005 | 0.200 | 2.10 | 7.61 |
| ft-xstrong | 1.000 | 0.990 | 1.62 | 3.42 | 0.17 | 0.00 | 0.280 | 0.004 | 0.230 | 2.17 | 7.72 |
| A* | 0.960 | 0.956 | 0.84 | 2.54 | 0.00 | 0.01 | 0.160 | 0.003 | 0.235 | 0.07 | 10.25 |

