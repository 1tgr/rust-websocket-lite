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

function test_autobahn_client() {
  app=$1; shift;

  RUST_BACKTRACE=1 ${app} ws://fuzzingserver:9001
  sed -i -e "/duration/d" reports/clients/index.json
  diff {/tmp/,}reports/clients/index.json
}

test_hello_world python-hello-world-server 8765
test_hello_world hyper-hello-world-server 9001

cp -R reports /tmp/

until nc -z fuzzingserver 9001; do
  sleep 1
done

test_autobahn_client ./async-autobahn-client
test_autobahn_client ./autobahn-client

until nc -z hyper-autobahn-server 9001; do
  sleep 1
done

wstest -m fuzzingclient --spec config/fuzzingclient.json
sed -i -e "/duration/d" reports/servers/index.json
diff {/tmp/,}reports/servers/index.json
