// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {Script, console2} from "forge-std/Script.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {BerylTips} from "../src/BerylTips.sol";
import {BerylPaidPosts} from "../src/BerylPaidPosts.sol";

/// @notice Phase W-2: deploys BerylTips and BerylPaidPosts on Sepolia. The deployer is the
///         owner and the treasury; the fee starts at 0. Sepolia only. Run from contracts/
///         without --broadcast to check (docs/contracts.md: Sepolia now needs the network's own
///         gas estimate for contract creation, so the real deployment uses `cast send --create`).
contract DeployCreator is Script {
    uint256 internal constant SEPOLIA = 11155111;
    /// @dev Circle's test USDC on Sepolia (lib/beryva/assets.ts).
    address internal constant USDC = 0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238;

    function run() external returns (BerylTips tips, BerylPaidPosts posts) {
        require(block.chainid == SEPOLIA, "Beryl creator contracts deploy to Sepolia only");
        uint256 key = vm.envUint("BERYVA_DEPLOYER_PRIVATE_KEY");
        address deployer = vm.addr(key);
        vm.startBroadcast(key);
        tips = new BerylTips(deployer, deployer, IERC20(USDC));
        posts = new BerylPaidPosts(deployer, deployer, IERC20(USDC));
        vm.stopBroadcast();
        console2.log("BerylTips", address(tips));
        console2.log("BerylPaidPosts", address(posts));
    }
}
