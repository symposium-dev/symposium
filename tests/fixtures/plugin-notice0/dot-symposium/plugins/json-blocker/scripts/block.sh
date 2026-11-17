#!/bin/sh
case "$(cat)" in
*"block with json"* | *"block, then lift it"*)
  echo '{"decision":"block","reason":"blocked by the json-blocker plugin"}'
  ;;
esac
