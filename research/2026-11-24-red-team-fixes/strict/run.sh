#!/bin/bash
# Build the strict reader, start the four HTTP parsers, run the corpus through all of them.
# usage: run.sh   (needs Zig 0.15.2, Go, Node, Python with h11, zlib and zstd headers)
H=$(cd $(dirname $0); pwd); B=${BIN:-/tmp/strict-bin}; mkdir -p $B
(cd $H && zig test strict.zig -lc -lz -lzstd && zig build-exe check.zig -lc -lz -lzstd -O ReleaseSafe -femit-bin=$B/strictcheck) || exit 1
(cd $H/parsers && go build -o $B/go_server go_server.go && go build -o $B/json_go json_go.go) || exit 1
(cd $H && python3 cases.py)
node $H/parsers/node_server.js & A=$!; $B/go_server & G=$!; python3 $H/parsers/h11_server.py & P=$!; python3 $H/parsers/lenient_server.py & L=$!
sleep 1
JSON_GO=$B/json_go python3 $H/diff.py $B/strictcheck
kill $A $G $P $L
