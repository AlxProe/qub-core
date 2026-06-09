// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {JinexGold} from "./JinexGold.sol";

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

/// @title XAUJ pooled reserve vault
/// @notice Infuse PAXG/XAUt into one fungible XAUJ token; melt XAUJ into either reserve bucket if liquidity exists.
contract XAUJReserveVault {
    using SafeToken for IERC20Like;

    enum BackingAsset { PAXG, XAUT }

    JinexGold public immutable xauj;
    IERC20Like public immutable paxg;
    IERC20Like public immutable xaut;

    address public owner;
    address public pendingOwner;
    bool public paused;

    uint256 public constant XAUT_TO_XAUJ_SCALE = 1_000_000_000_000; // XAUt has 6 decimals; XAUJ/PAXG use 18.

    uint256 public paxgReserveUnits; // native PAXG units (18 decimals)
    uint256 public xautReserveUnits; // native XAUt units (6 decimals)

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
    error NonMicroXAUJAmountForXAUTRedemption();
    error CannotRescueReserveOrXAUJ();

    uint256 private locked = 1;

    modifier onlyOwner() { if (msg.sender != owner) revert NotOwner(); _; }
    modifier whenNotPaused() { if (paused) revert PausedError(); _; }
    modifier nonReentrant() { if (locked != 1) revert Reentrant(); locked = 2; _; locked = 1; }

    constructor(address xauj_, address paxg_, address xaut_, address owner_) {
        if (xauj_ == address(0) || paxg_ == address(0) || xaut_ == address(0) || owner_ == address(0)) revert ZeroAddress();
        xauj = JinexGold(xauj_);
        paxg = IERC20Like(paxg_);
        xaut = IERC20Like(xaut_);
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

    function pause() external onlyOwner { paused = true; emit Paused(msg.sender); }
    function unpause() external onlyOwner { paused = false; emit Unpaused(msg.sender); }

    /// @notice Deposit PAXG/XAUt and mint one fungible XAUJ. Asset id: 0=PAXG, 1=XAUt.
    function infuse(uint8 asset, uint256 amount, address receiver) external nonReentrant whenNotPaused {
        if (amount == 0) revert InvalidAmount();
        if (receiver == address(0)) revert ZeroAddress();
        IERC20Like token = _token(asset);
        uint256 beforeBal = token.balanceOf(address(this));
        token.safeTransferFrom(msg.sender, address(this), amount);
        uint256 received = token.balanceOf(address(this)) - beforeBal;
        if (received != amount) revert FeeOnTransferOrUnexpectedTokenBehavior();
        uint256 xaujAmount;
        if (asset == uint8(BackingAsset.PAXG)) {
            paxgReserveUnits += amount;
            xaujAmount = amount;
            emit Infused(msg.sender, receiver, BackingAsset.PAXG, amount);
        } else if (asset == uint8(BackingAsset.XAUT)) {
            xautReserveUnits += amount;
            xaujAmount = amount * XAUT_TO_XAUJ_SCALE;
            emit Infused(msg.sender, receiver, BackingAsset.XAUT, amount);
        } else {
            revert InvalidAsset();
        }
        xauj.mintFromVault(receiver, xaujAmount);
    }

    /// @notice Burn XAUJ and redeem selected pooled reserve if liquidity exists. Asset id: 0=PAXG, 1=XAUt.
    function melt(uint8 asset, uint256 amount, address receiver) external nonReentrant whenNotPaused {
        if (amount == 0) revert InvalidAmount();
        if (receiver == address(0)) revert ZeroAddress();
        IERC20Like token = _token(asset);
        uint256 redeemNativeAmount;
        if (asset == uint8(BackingAsset.PAXG)) {
            redeemNativeAmount = amount;
            if (paxgReserveUnits < redeemNativeAmount || token.balanceOf(address(this)) < redeemNativeAmount) revert InsufficientReserve();
            paxgReserveUnits -= redeemNativeAmount;
            emit Melted(msg.sender, receiver, BackingAsset.PAXG, redeemNativeAmount);
        } else if (asset == uint8(BackingAsset.XAUT)) {
            if (amount % XAUT_TO_XAUJ_SCALE != 0) revert NonMicroXAUJAmountForXAUTRedemption();
            redeemNativeAmount = amount / XAUT_TO_XAUJ_SCALE;
            if (xautReserveUnits < redeemNativeAmount || token.balanceOf(address(this)) < redeemNativeAmount) revert InsufficientReserve();
            xautReserveUnits -= redeemNativeAmount;
            emit Melted(msg.sender, receiver, BackingAsset.XAUT, redeemNativeAmount);
        } else {
            revert InvalidAsset();
        }
        xauj.burnFromVault(msg.sender, amount);
        token.safeTransfer(receiver, redeemNativeAmount);
    }

    function reserveOf(uint8 asset) external view returns (uint256) {
        if (asset == uint8(BackingAsset.PAXG)) return paxgReserveUnits;
        if (asset == uint8(BackingAsset.XAUT)) return xautReserveUnits;
        revert InvalidAsset();
    }

    function maxRedeemable(uint8 asset) external view returns (uint256) {
        if (asset == uint8(BackingAsset.PAXG)) return _min(paxgReserveUnits, paxg.balanceOf(address(this)));
        if (asset == uint8(BackingAsset.XAUT)) return _min(xautReserveUnits, xaut.balanceOf(address(this)));
        revert InvalidAsset();
    }

    function backingSummary() external view returns (uint256 paxgReserve, uint256 xautReserve, uint256 totalBacking, uint256 totalLiability) {
        paxgReserve = paxgReserveUnits;
        xautReserve = xautReserveUnits;
        totalBacking = paxgReserve + (xautReserve * XAUT_TO_XAUJ_SCALE);
        totalLiability = xauj.totalSupply();
    }

    /// @notice Rescue only tokens that are not XAUJ/PAXG/XAUt. No owner withdrawal power over reserves.
    function rescueUnsupportedToken(address token, address to, uint256 amount) external onlyOwner nonReentrant {
        if (token == address(xauj) || token == address(paxg) || token == address(xaut)) revert CannotRescueReserveOrXAUJ();
        if (to == address(0)) revert ZeroAddress();
        IERC20Like(token).safeTransfer(to, amount);
        emit UnsupportedTokenRescued(token, to, amount);
    }

    function _token(uint8 asset) internal view returns (IERC20Like) {
        if (asset == uint8(BackingAsset.PAXG)) return paxg;
        if (asset == uint8(BackingAsset.XAUT)) return xaut;
        revert InvalidAsset();
    }

    function _min(uint256 a, uint256 b) internal pure returns (uint256) { return a < b ? a : b; }
}
