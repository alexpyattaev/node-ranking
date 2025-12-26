# Node Ranking

A tool to fetch Solana cluster nodes.

Uses Gossip protocol to fetch live info about IPs and such, and RPC to fetch info about stakes (as that changes very rarely).

Make sure you run this yourself and never rely on someone else's instance.

# Cluster restarts

This tool will not handle restarts for you. You will need to restart the tool manually and ensure it latches onto the correct shred_version.
