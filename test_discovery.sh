#!/bin/bash
cd /Users/nikomat/dev/worktrees/symposium/sure-trail/symposium
cargo test plugins::tests::scan_source_dir --lib -- --nocapture