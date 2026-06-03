// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

/// @title Jinex USD (USDJ)
/// @notice Single public Ethereum-side USDJ token. Mint/burn is restricted to the configured reserve vault.
/// @dev Self-contained ERC-20 implementation to avoid external dependencies in the source package.
contract JinexUSD {
    string public name;
    string public symbol;
    uint8 public constant decimals = 6;

    uint256 public totalSupply;
    address public owner;
    address public pendingOwner;
    address public vaultMinter;
    bool public vaultConfigured;

    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);
    event OwnershipTransferStarted(address indexed previousOwner, address indexed newOwner);
    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);
    event VaultMinterConfigured(address indexed vault);

    error ZeroAddress();
    error NotOwner();
    error NotPendingOwner();
    error VaultAlreadyConfigured();
    error NotVault();
    error InsufficientBalance();
    error InsufficientAllowance();

    modifier onlyOwner() {
        if (msg.sender != owner) revert NotOwner();
        _;
    }

    modifier onlyVault() {
        if (msg.sender != vaultMinter) revert NotVault();
        _;
    }

    constructor(string memory name_, string memory symbol_, address owner_) {
        if (owner_ == address(0)) revert ZeroAddress();
        name = name_;
        symbol = symbol_;
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

    /// @notice One-way vault configuration. After this, only the vault can mint/burn.
    function configureVaultMinter(address vault) external onlyOwner {
        if (vaultConfigured) revert VaultAlreadyConfigured();
        if (vault == address(0)) revert ZeroAddress();
        vaultConfigured = true;
        vaultMinter = vault;
        emit VaultMinterConfigured(vault);
    }

    function approve(address spender, uint256 value) external returns (bool) {
        allowance[msg.sender][spender] = value;
        emit Approval(msg.sender, spender, value);
        return true;
    }

    function transfer(address to, uint256 value) external returns (bool) {
        _transfer(msg.sender, to, value);
        return true;
    }

    function transferFrom(address from, address to, uint256 value) external returns (bool) {
        uint256 allowed = allowance[from][msg.sender];
        if (allowed != type(uint256).max) {
            if (allowed < value) revert InsufficientAllowance();
            unchecked { allowance[from][msg.sender] = allowed - value; }
            emit Approval(from, msg.sender, allowance[from][msg.sender]);
        }
        _transfer(from, to, value);
        return true;
    }

    function mintFromVault(address to, uint256 value) external onlyVault {
        if (to == address(0)) revert ZeroAddress();
        totalSupply += value;
        balanceOf[to] += value;
        emit Transfer(address(0), to, value);
    }

    function burnFromVault(address from, uint256 value) external onlyVault {
        if (from == address(0)) revert ZeroAddress();
        uint256 bal = balanceOf[from];
        if (bal < value) revert InsufficientBalance();
        unchecked { balanceOf[from] = bal - value; }
        totalSupply -= value;
        emit Transfer(from, address(0), value);
    }

    function _transfer(address from, address to, uint256 value) internal {
        if (to == address(0)) revert ZeroAddress();
        uint256 bal = balanceOf[from];
        if (bal < value) revert InsufficientBalance();
        unchecked { balanceOf[from] = bal - value; }
        balanceOf[to] += value;
        emit Transfer(from, to, value);
    }
}
