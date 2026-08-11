// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import { Script, console } from "forge-std/Script.sol";
import { BotchainNNMesh } from "../src/BotchainNNMesh.sol";

/**
 * @notice Deploys `BotchainNNMesh` only.
 *
 * Separate from `Deploy.s.sol` on purpose. That script deploys the arb executor and the
 * bond escrow, and both are already live at nonce 0 and 1 — re-running it would deploy a
 * second copy of each at new addresses, quietly orphaning the ones in use. A script that
 * can only deploy the thing you are actually deploying cannot make that mistake.
 *
 * ```
 * forge script script/DeployMesh.s.sol --rpc-url botchain --account botchain-deployer --legacy
 * forge script script/DeployMesh.s.sol --rpc-url botchain --account botchain-deployer --legacy --broadcast
 * ```
 */
contract DeployMesh is Script {
    uint256 internal constant BOTCHAIN_MAINNET = 677;
    uint256 internal constant BOTCHAIN_TESTNET = 968;

    function run() external {
        address owner = vm.envAddress("TREASURY");
        require(owner != address(0), "TREASURY must be set");
        require(
            block.chainid == BOTCHAIN_MAINNET || block.chainid == BOTCHAIN_TESTNET,
            "refusing to deploy: not a BOT Chain network"
        );

        console.log("chain id :", block.chainid);
        console.log("owner    :", owner);

        vm.startBroadcast();
        BotchainNNMesh mesh = new BotchainNNMesh(owner);
        vm.stopBroadcast();

        console.log("BotchainNNMesh :", address(mesh));
        console.log("");
        console.log("Next, separately, after verifying source:");
        console.log("  mesh.setArbiter(<arbiter>, true)");
        console.log("  mesh.registerAgent(<weightsHash>, <uri>)");
    }
}
