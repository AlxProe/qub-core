// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

import {JinexEUR} from "./JinexEUR.sol";

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

/// @title EURJ pooled reserve vault
/// @notice Infuse EURC/EURS into one fungible EURJ token; melt EURJ into either reserve bucket if liquidity exists.
contract EURJReserveVault {
    using SafeToken for IERC20Like;

    enum BackingAsset { EURC, EURS }

    JinexEUR public immutable eurj;
    IERC20Like public immutable eurc;
    IERC20Like public immutable eurs;

    address public owner;
    address public pendingOwner;
    bool public paused;

    uint256 public constant EURS_TO_EURJ_SCALE = 10_000; // EURS has 2 decimals; EURJ/EURC use 6.

    uint256 public eurcReserveUnits; // native EURC units (6 decimals)
    uint256 public eursReserveUnits; // native EURS units (2 decimals)

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
    error NonCentEURJAmountForEURSRedemption();
    error CannotRescueReserveOrEURJ();

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

    constructor(address eurj_, address eurc_, address eurs_, address owner_) {
        if (eurj_ == address(0) || eurc_ == address(0) || eurs_ == address(0) || owner_ == address(0)) revert ZeroAddress();
        eurj = JinexEUR(eurj_);
        eurc = IERC20Like(eurc_);
        eurs = IERC20Like(eurs_);
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

    /// @notice Deposit EURC/EURS and mint one fungible EURJ. Asset id: 0=EURC, 1=EURS.
    function infuse(uint8 asset, uint256 amount, address receiver) external nonReentrant whenNotPaused {
        if (amount == 0) revert InvalidAmount();
        if (receiver == address(0)) revert ZeroAddress();
        IERC20Like token = _token(asset);
        uint256 beforeBal = token.balanceOf(address(this));
        token.safeTransferFrom(msg.sender, address(this), amount);
        uint256 received = token.balanceOf(address(this)) - beforeBal;
        if (received != amount) revert FeeOnTransferOrUnexpectedTokenBehavior();
        uint256 eurjAmount;
        if (asset == uint8(BackingAsset.EURC)) {
            eurcReserveUnits += amount;
            eurjAmount = amount;
            emit Infused(msg.sender, receiver, BackingAsset.EURC, amount);
        } else if (asset == uint8(BackingAsset.EURS)) {
            eursReserveUnits += amount;
            eurjAmount = amount * EURS_TO_EURJ_SCALE;
            emit Infused(msg.sender, receiver, BackingAsset.EURS, amount);
        } else {
            revert InvalidAsset();
        }
        eurj.mintFromVault(receiver, eurjAmount);
    }

    /// @notice Burn EURJ and redeem selected pooled reserve if liquidity exists. Asset id: 0=EURC, 1=EURS.
    function melt(uint8 asset, uint256 amount, address receiver) external nonReentrant whenNotPaused {
        if (amount == 0) revert InvalidAmount();
        if (receiver == address(0)) revert ZeroAddress();
        IERC20Like token = _token(asset);
        uint256 redeemNativeAmount;
        if (asset == uint8(BackingAsset.EURC)) {
            redeemNativeAmount = amount;
            if (eurcReserveUnits < redeemNativeAmount || token.balanceOf(address(this)) < redeemNativeAmount) revert InsufficientReserve();
            eurcReserveUnits -= redeemNativeAmount;
            emit Melted(msg.sender, receiver, BackingAsset.EURC, redeemNativeAmount);
        } else if (asset == uint8(BackingAsset.EURS)) {
            if (amount % EURS_TO_EURJ_SCALE != 0) revert NonCentEURJAmountForEURSRedemption();
            redeemNativeAmount = amount / EURS_TO_EURJ_SCALE;
            if (eursReserveUnits < redeemNativeAmount || token.balanceOf(address(this)) < redeemNativeAmount) revert InsufficientReserve();
            eursReserveUnits -= redeemNativeAmount;
            emit Melted(msg.sender, receiver, BackingAsset.EURS, redeemNativeAmount);
        } else {
            revert InvalidAsset();
        }
        eurj.burnFromVault(msg.sender, amount);
        token.safeTransfer(receiver, redeemNativeAmount);
    }

    function reserveOf(uint8 asset) external view returns (uint256) {
        if (asset == uint8(BackingAsset.EURC)) return eurcReserveUnits;
        if (asset == uint8(BackingAsset.EURS)) return eursReserveUnits;
        revert InvalidAsset();
    }

    function maxRedeemable(uint8 asset) external view returns (uint256) {
        if (asset == uint8(BackingAsset.EURC)) return _min(eurcReserveUnits, eurc.balanceOf(address(this)));
        if (asset == uint8(BackingAsset.EURS)) return _min(eursReserveUnits, eurs.balanceOf(address(this)));
        revert InvalidAsset();
    }

    function backingSummary() external view returns (uint256 eurcReserve, uint256 eursReserve, uint256 totalBacking, uint256 totalLiability) {
        eurcReserve = eurcReserveUnits;
        eursReserve = eursReserveUnits;
        totalBacking = eurcReserve + (eursReserve * EURS_TO_EURJ_SCALE);
        totalLiability = eurj.totalSupply();
    }

    /// @notice Rescue only tokens that are not EURJ/EURC/EURS. No owner withdrawal power over reserves.
    function rescueUnsupportedToken(address token, address to, uint256 amount) external onlyOwner nonReentrant {
        if (token == address(eurj) || token == address(eurc) || token == address(eurs)) revert CannotRescueReserveOrEURJ();
        if (to == address(0)) revert ZeroAddress();
        IERC20Like(token).safeTransfer(to, amount);
        emit UnsupportedTokenRescued(token, to, amount);
    }

    function _token(uint8 asset) internal view returns (IERC20Like) {
        if (asset == uint8(BackingAsset.EURC)) return eurc;
        if (asset == uint8(BackingAsset.EURS)) return eurs;
        revert InvalidAsset();
    }

    function _min(uint256 a, uint256 b) internal pure returns (uint256) { return a < b ? a : b; }
}
