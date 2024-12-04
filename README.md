# oort3 bencher
A small program that lets you test performance improvements for your oort3 AI locally.

## Installation
```shell
git clone --recursive https://github.com/arihant2math/oort3_bencher.git
cd oort3_bencher
cargo build --release
```
## Usage
```shell
oort3_bencher [path-to-baseline] [path-to-new] "tutorial_frigate,tutorial_cruiser"
```
```shell
oort3_bencher [path-to-baseline] [path-to-new] [path-to-benchmark-list]
```
The benchmark list is a file containing a list of benchmarks to run. Each line should contain a benchmark:

```
# Commented lines are ignored
tutorial_squadron
tutorial_frigate
tutorial_cruiser
fighter_duel
frigate_duel
cruiser_duel
fleet
```
