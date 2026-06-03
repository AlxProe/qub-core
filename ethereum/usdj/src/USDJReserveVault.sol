// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {JinexUSD} from "./JinexUSD.sol";

interface IERC20Like {
    function balanceOf(address account) external view returns (uint256);
    function transfer(address to, uint256 value) external returns (bool);
    function transferFrom(address from, address to, uint256 value) external returns (bool);
}

library SafeToken {
    error TokenCallFailed();
    error TokenOperationFailed();

    function safeTransfer(IERC20Like token, address to, uint256 value) internal {
        _callOptionalReturn(address(token), abi.encodeWithSelector(token.transfer.selector, to, value));
    }

    function safeTransferFrom(IERC20Like token, address from, address to, uint256 value) internal {
        _callOptionalReturn(address(token), abi.encodeWithSelector(token.transferFrom.selector, from, to, value));
    }

    function _callOptionalReturn(address token, bytes memory data) private {
        (bool ok, bytes memory ret) = token.call(data);
        if (!ok) revert TokenCallFailed();
        if (ret.length != 0 && !abi.decode(ret, (bool))) revert TokenOperationFailed();
    }
}

/// @title USDJ pooled reserve vault
/// @notice Infuse USDT/USDC into one fungible USDJ token; melt USDJ into either reserve bucket if liquidity exists.
contract USDJReserveVault {
    using SafeToken for IERC20Like;

    enum BackingAsset { USDT, USDC }

    JinexUSD public immutable usdj;
    IERC20Like public immutable usdt;
    IERC20Like public immutable usdc;

    address public owner;
    address public pendingOwner;
    bool public paused;

    uint256 public usdtReserveUnits;
    uint256 public usdcReserveUnits;

    event OwnershipTransferStarted(address indexed previousOwner, address indexed newOwner);
    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);
    event Paused(address indexed by);
    event Unpaused(address indexed by);
    event Infused(address indexed account, address indexed receiver, BackingAsset indexed asset, uint256 amount);
    event Melted(address indexed account, address indexed receiver, BackingAsset indexed asset, uint256 amount);
    event UnsupportedTokenRescued(address indexed token, address indexed to, uint256 amount);

    error ZeroAddress();
    error NotOwner();
    error NotPendingOwner();
    error PausedError();
    error Reentrant();
    error InvalidAmount();
    error InvalidAsset();
    error FeeOnTransferOrUnexpectedTokenBehavior();
    error InsufficientReserve();
    error CannotRescueReserveOrUSDJ();

    uint256 private locked = 1;

    modifier onlyOwner() {
        if (msg.sender != owner) revert NotOwner();
        _;
    }

    modifier whenNotPaused() {
        if (paused) revert PausedError();
        _;
    }

    modifier nonReentrant() {
        if (locked != 1) revert Reentrant();
        locked = 2;
        _;
        locked = 1;
    }

    constructor(address usdj_, address usdt_, address usdc_, address owner_) {
        if (usdj_ == address(0) || usdt_ == address(0) || usdc_ == address(0) || owner_ == address(0)) revert ZeroAddress();
        usdj = JinexUSD(usdj_);
        usdt = IERC20Like(usdt_);
        usdc = IERC20Like(usdc_);
        owner = owner_;
        emit OwnershipTransferred(address(0), owner_);
    }

    function transferOwnership(address newOwner) external onlyOwner {
        if (newOwner == address(0)) revert ZeroAddress();
        pendingOwner = newOwner;
        emit OwnershipTransferStarted(owner, newOwner);
    }

    function acceptOwnership() external {
        if (msg.sender != pendingOwner) revert NotPendingOwner();
        address old = owner;
        owner = pendingOwner;
        pendingOwner = address(0);
        emit OwnershipTransferred(old, owner);
    }

    function pause() external onlyOwner {
        paused = true;
        emit Paused(msg.sender);
    }

    function unpause() external onlyOwner {
        paused = false;
        emit Unpaused(msg.sender);
    }

    /// @notice Deposit USDT/USDC and mint one fungible USDJ. Asset id: 0=USDT, 1=USDC.
    function infuse(uint8 asset, uint256 amount, address receiver) external nonReentrant whenNotPaused {
        if (amount == 0) revert InvalidAmount();
        if (receiver == address(0)) revert ZeroAddress();
        IERC20Like token = _token(asset);
        uint256 beforeBal = token.balanceOf(address(this));
        token.safeTransferFrom(msg.sender, address(this), amount);
        uint256 received = token.balanceOf(address(this)) - beforeBal;
        if (received != amount) revert FeeOnTransferOrUnexpectedTokenBehavior();
        if (asset == uint8(BackingAsset.USDT)) {
            usdtReserveUnits += amount;
            emit Infused(msg.sender, receiver, BackingAsset.USDT, amount);
        } else if (asset == uint8(BackingAsset.USDC)) {
            usdcReserveUnits += amount;
            emit Infused(msg.sender, receiver, BackingAsset.USDC, amount);
        } else {
            revert InvalidAsset();
        }
        usdj.mintFromVault(receiver, amount);
    }

    /// @notice Burn USDJ and redeem selected pooled reserve if liquidity exists. Asset id: 0=USDT, 1=USDC.
    function melt(uint8 asset, uint256 amount, address receiver) external nonReentrant whenNotPaused {
        if (amount == 0) revert InvalidAmount();
        if (receiver == address(0)) revert ZeroAddress();
        IERC20Like token = _token(asset);
        if (asset == uint8(BackingAsset.USDT)) {
            if (usdtReserveUnits < amount || token.balanceOf(address(this)) < amount) revert InsufficientReserve();
            usdtReserveUnits -= amount;
            emit Melted(msg.sender, receiver, BackingAsset.USDT, amount);
        } else if (asset == uint8(BackingAsset.USDC)) {
            if (usdcReserveUnits < amount || token.balanceOf(address(this)) < amount) revert InsufficientReserve();
            usdcReserveUnits -= amount;
            emit Melted(msg.sender, receiver, BackingAsset.USDC, amount);
        } else {
            revert InvalidAsset();
        }
        usdj.burnFromVault(msg.sender, amount);
        token.safeTransfer(receiver, amount);
    }

    function reserveOf(uint8 asset) external view returns (uint256) {
        if (asset == uint8(BackingAsset.USDT)) return usdtReserveUnits;
        if (asset == uint8(BackingAsset.USDC)) return usdcReserveUnits;
        revert InvalidAsset();
    }

    function maxRedeemable(uint8 asset) external view returns (uint256) {
        if (asset == uint8(BackingAsset.USDT)) return _min(usdtReserveUnits, usdt.balanceOf(address(this)));
        if (asset == uint8(BackingAsset.USDC)) return _min(usdcReserveUnits, usdc.balanceOf(address(this)));
        revert InvalidAsset();
    }

    function backingSummary() external view returns (uint256 usdtReserve, uint256 usdcReserve, uint256 totalBacking, uint256 totalLiability) {
        usdtReserve = usdtReserveUnits;
        usdcReserve = usdcReserveUnits;
        totalBacking = usdtReserve + usdcReserve;
        totalLiability = usdj.totalSupply();
    }

    /// @notice Rescue only tokens that are not USDJ/USDT/USDC. No owner withdrawal power over reserves.
    function rescueUnsupportedToken(address token, address to, uint256 amount) external onlyOwner nonReentrant {
        if (token == address(usdj) || token == address(usdt) || token == address(usdc)) revert CannotRescueReserveOrUSDJ();
        if (to == address(0)) revert ZeroAddress();
        IERC20Like(token).safeTransfer(to, amount);
        emit UnsupportedTokenRescued(token, to, amount);
    }

    function _token(uint8 asset) internal view returns (IERC20Like) {
        if (asset == uint8(BackingAsset.USDT)) return usdt;
        if (asset == uint8(BackingAsset.USDC)) return usdc;
        revert InvalidAsset();
    }

    function _min(uint256 a, uint256 b) internal pure returns (uint256) { return a < b ? a : b; }
}
