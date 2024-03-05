nohup cargo run --bin driver -- --ethrpc "http://0.0.0.0:8545" --config "crates/driver/driver_config_prod.toml" &
echo "DRIVER_PID="$! > driver_pid.log