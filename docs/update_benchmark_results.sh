#!/usr/bin/env bash

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

cp "$SCRIPT_DIR/../target/criterion/Primes2000/(Nums=2000 Parallel=8 Same=true)/report/violin.svg" "$SCRIPT_DIR/benchmark_results/primes2000same_p8_violin.svg"
cp "$SCRIPT_DIR/../target/criterion/Primes2000/(Nums=2000 Parallel=1 Same=true)/report/violin.svg" "$SCRIPT_DIR/benchmark_results/primes2000same_p1_violin.svg"
cp "$SCRIPT_DIR/../target/criterion/Primes2000/(Nums=2000 Parallel=1 Same=false)/report/violin.svg" "$SCRIPT_DIR/benchmark_results/primes2000_p1_violin.svg"
cp "$SCRIPT_DIR/../target/criterion/Primes2000/(Nums=2000 Parallel=8 Same=false)/report/violin.svg" "$SCRIPT_DIR/benchmark_results/primes2000_p8_violin.svg"
