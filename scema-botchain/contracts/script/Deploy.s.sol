// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import { Script, console } from "forge-std/Script.sol";
import { ScemaArbExecutor } from "../src/ScemaArbExecutor.sol";
import { ScemaBondEscrow } from "../src/ScemaBondEscrow.sol";

/**
 * @notice Deploys the arb executor and the bond escrow.
 *
 * ```
 * forge script script/Deploy.s.sol --rpc-url botchain --account botchain-deployer --legacy          # dry run
 * forge script script/Deploy.s.sol --rpc-url botchain --account botchain-deployer --legacy --broadcast
 * ```
 *
 * `--legacy` is required, not stylistic: BOT Chain reports `baseFeePerGas = 0`, so an
 * EIP-1559 transaction is priced at zero priority and validators have no reason to
 * include it.
 *
 * # What this deliberately does not do
 *
 * It deploys the contracts and stops. It does **not** whitelist venues, grant executor
 * rights, or set approvals — every one of those is a decision about who can move money,
 * and batching them into a deploy means they happen before anyone has looked at the
 * deployed bytecode on the explorer. Run them afterwards, individually, against a
 * verified contract.
 */
contract Deploy is Script {
    /// Guards against deploying to a chain you did not mean to. 677 = BOT Chain mainnet.
    uint256 internal constant BOTCHAIN_MAINNET = 677;
    uint256 internal constant BOTCHAIN_TESTNET = 968;

    function run() external {
        // The owner ends up holding every privileged role. A wrong value here is not
        // fixable after the fact, so it is required rather than defaulted to msg.sender:
        // an accidental deploy owned by a throwaway key is worse than a failed script.
        address owner = vm.envAddress("TREASURY");
        require(owner != address(0), "TREASURY must be set");

        uint256 chainId = block.chainid;
        require(
            chainId == BOTCHAIN_MAINNET || chainId == BOTCHAIN_TESTNET,
            "refusing to deploy: not a BOT Chain network"
        );

        console.log("chain id :", chainId);
        console.log("owner    :", owner);

        vm.startBroadcast();

        ScemaArbExecutor executor = new ScemaArbExecutor(owner);
        ScemaBondEscrow escrow = new ScemaBondEscrow(owner);

        vm.stopBroadcast();

        console.log("ScemaArbExecutor :", address(executor));
        console.log("ScemaBondEscrow  :", address(escrow));
        console.log("");
        console.log("Next, separately and only after verifying source on the explorer:");
        console.log("  executor.setTarget(<router>, true)");
        console.log("  executor.setExecutor(<bot key>, true)");
        console.log("  executor.setApproval(<token>, <router>, <amount>)");
        console.log("  escrow.setArbiter(<arbiter>, true)");
    }
}
