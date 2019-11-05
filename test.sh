#!/bin/bash
set -xeuo pipefail

echo '{"op": "subscribe", "args": ["orderBookL2:XBTUSD"]}' | ./wsdump --eof-wait 5 wss://www.bitmex.com/realtime > /dev/null

until nc -z fuzzingserver 9001 && nc -z hello-world-server 8765; do
  sleep 1
done

[ "$(./hello-world-client ws://hello-world-server:8765)" = "Hello, world!" ]

[ "$(./wsdump --eof-wait 1 ws://hello-world-server:8765 < /dev/null)" = "Hello, world!" ]

cp /reports/clients/index.json /tmp/report.json

RUST_BACKTRACE=1 ./async-autobahn-client ws://fuzzingserver:9001
diff /tmp/report.json <(grep -v duration /reports/clients/index.json)

RUST_BACKTRACE=1 ./autobahn-client ws://fuzzingserver:9001
diff /tmp/report.json <(grep -v duration /reports/clients/index.json)
