#!/usr/bin/env bash

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

cp $SCRIPT_DIR/../target/criterion/reports/Primes2000/violin.svg $SCRIPT_DIR/benchmark_results/primes2000_violin.svg
cp $SCRIPT_DIR/../target/criterion/reports/Primes2000Same/violin.svg $SCRIPT_DIR/benchmark_results/primes2000same_violin.svg
