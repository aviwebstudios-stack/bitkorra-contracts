// SPDX-License-Identifier: MIT
pragma solidity 0.8.30;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";

/// @notice A plain test token with chosen decimals (6 for a USDC stand-in).
contract MockERC20 is ERC20 {
    uint8 private immutable _decimals;

    constructor(string memory name_, string memory symbol_, uint8 decimals_) ERC20(name_, symbol_) {
        _decimals = decimals_;
    }

    function decimals() public view override returns (uint8) {
        return _decimals;
    }

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }
}

/// @notice WETH9's behaviour (deposit, withdraw, 1:1 with ETH), for tests.
contract MockWETH is ERC20 {
    constructor() ERC20("Wrapped Ether", "WETH") {}

    receive() external payable {
        deposit();
    }

    function deposit() public payable {
        _mint(msg.sender, msg.value);
    }

    function withdraw(uint256 amount) external {
        _burn(msg.sender, amount);
        (bool ok,) = msg.sender.call{value: amount}("");
        require(ok, "WETH: ETH transfer failed");
    }
}

/// @notice Tries to re-enter the router when it receives ETH.
contract ReentrantReceiver {
    address public immutable router;
    bool public reentered;
    bool public reentryBlocked;

    constructor(address router_) {
        router = router_;
    }

    receive() external payable {
        if (reentered) return;
        reentered = true;
        address[] memory path = new address[](0);
        (bool ok,) = router.call(abi.encodeWithSignature("swapExactETHForTokens(uint256,address[],address,uint256)", 0, path, address(this), block.timestamp));
        reentryBlocked = !ok;
    }
}

/// @notice Refuses ETH.
contract EthRefuser {
    receive() external payable {
        revert("no ETH");
    }
}

/// @notice Phase W-2: a creator wallet that refuses ETH (a contract with no receive).
contract RejectingCreator {}

/// @notice Phase W-2: a creator that tries to re-enter when it receives ETH, with any call.
contract ReentrantCreator {
    address public target;
    bytes public reentryCall;
    bool public reentered;
    bool public reentryBlocked;

    function arm(address target_, bytes calldata call_) external {
        target = target_;
        reentryCall = call_;
    }

    receive() external payable {
        if (reentered || target == address(0)) return;
        reentered = true;
        (bool ok,) = target.call{value: msg.value}(reentryCall);
        reentryBlocked = !ok;
    }
}

/// @notice Phase I-8: a USDC-like token that refuses transfers to blacklisted addresses (as USDC can).
contract BlacklistToken is ERC20 {
    mapping(address => bool) public blacklisted;

    constructor() ERC20("Blacklist USD", "BUSD") {}

    function decimals() public pure override returns (uint8) {
        return 6;
    }

    function mint(address to, uint256 amount) external {
        _mint(to, amount);
    }

    function setBlacklisted(address who, bool on) external {
        blacklisted[who] = on;
    }

    function _update(address from, address to, uint256 value) internal override {
        require(!blacklisted[to] && !blacklisted[from], "blacklisted");
        super._update(from, to, value);
    }
}
