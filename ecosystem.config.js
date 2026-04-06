module.exports = {
  apps: [{
    name: "samp-mirror",
    script: "./target/release/samp-mirror",
    args: "--node ws://127.0.0.1:9944 --db mirror.db --port 8080",  // replace with your node URL
    restart_delay: 5000,
    max_restarts: 10,
  }]
};
