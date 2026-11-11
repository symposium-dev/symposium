#!/bin/sh
case "$(cat)" in
*"block with exit 2"*)
  echo 'blocked by the blocker plugin' >&2
  exit 2
  ;;
esac
