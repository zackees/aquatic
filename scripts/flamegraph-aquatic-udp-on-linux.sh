#!/bin/bash

# Profile
perf record --call-graph=dwarf,16384 -e cpu-clock -F 997 target/release/aquatic_udp

# Generate a local SVG; open flame.svg directly in a browser.
# No web server is needed to view a profiling artifact.
# Info: https://gist.github.com/dlaehnemann/df31787c41bd50c0fe223df07cf6eb89
perf script | stackcollapse-perf.pl | c++filt | flamegraph.pl > flame.svg