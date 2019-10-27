#!/bin/bash
set -xeuo pipefail

echo '{"op": "subscribe", "args": ["orderBookL2:XBTUSD"]}' | ./wsdump --eof-wait 5 wss://www.bitmex.com/realtime > /dev/null

until nc -z fuzzingserver 9001; do
  sleep 1
done

cp /reports/clients/index.json /tmp/report.json

RUST_BACKTRACE=1 ./async-autobahn-client ws://fuzzingserver:9001
diff /tmp/report.json <(grep -v duration /reports/clients/index.json)

RUST_BACKTRACE=1 ./autobahn-client ws://fuzzingserver:9001
diff /tmp/report.json <(grep -v duration /reports/clients/index.json)
