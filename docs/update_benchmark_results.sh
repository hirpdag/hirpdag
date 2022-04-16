#!/usr/bin/env bash

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

cp $SCRIPT_DIR/../target/criterion/reports/Primes8000/violin.svg $SCRIPT_DIR/benchmark_results/primes8000_violin.svg
cp $SCRIPT_DIR/../target/criterion/reports/Primes8000Same/violin.svg $SCRIPT_DIR/benchmark_results/primes8000same_violin.svg
