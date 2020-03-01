#!/bin/bash
set -xeuo pipefail

echo '{"op": "subscribe", "args": ["orderBookL2:XBTUSD"]}' | ./wsdump --eof-wait 5 wss://www.bitmex.com/realtime > /dev/null

function test_hello_world() {
  host=$1; shift;
  port=$1; shift;

  until nc -z ${host} ${port}; do
    sleep 1
  done

  [ "$(./hello-world-client ws://${host}:${port})" = "Hello, world!" ]
  [ "$(./wsdump --eof-wait 1 ws://${host}:${port} < /dev/null)" = "Hello, world!" ]
}

test_hello_world python-hello-world-server 8765
test_hello_world hyper-hello-world-server 9001

cp /reports/clients/index.json /tmp/report.json

until nc -z fuzzingserver 9001; do
  sleep 1
done

RUST_BACKTRACE=1 ./async-autobahn-client ws://fuzzingserver:9001
diff /tmp/report.json <(grep -v duration /reports/clients/index.json)

RUST_BACKTRACE=1 ./autobahn-client ws://fuzzingserver:9001
diff /tmp/report.json <(grep -v duration /reports/clients/index.json)
