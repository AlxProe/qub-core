// SPDX-License-Identifier: MIT
pragma solidity 0.8.28;

interface IUSDJBridgeToken {
    function balanceOf(address account) external view returns (uint256);
    function transfer(address to, uint256 value) external returns (bool);
    function transferFrom(address from, address to, uint256 value) external returns (bool);
}

interface IQubUsdJBurnProofVerifier {
    function verifyUsdJExit(bytes32 burnId, address receiver, uint256 amount, bytes calldata proof) external view returns (bool);
}

library BridgeSafeToken {
    error TokenCallFailed();
    error TokenOperationFailed();

    function safeTransfer(IUSDJBridgeToken token, address to, uint256 value) internal {
        _callOptionalReturn(address(token), abi.encodeWithSelector(token.transfer.selector, to, value));
    }

    function safeTransferFrom(IUSDJBridgeToken token, address from, address to, uint256 value) internal {
        _callOptionalReturn(address(token), abi.encodeWithSelector(token.transferFrom.selector, from, to, value));
    }

    function _callOptionalReturn(address token, bytes memory data) private {
        (bool ok, bytes memory ret) = token.call(data);
        if (!ok) revert TokenCallFailed();
        if (ret.length != 0 && !abi.decode(ret, (bool))) revert TokenOperationFailed();
    }
}

/// @title USDJ Bridge Gateway scaffold
/// @notice Locks Ethereum USDJ for future QUB-chain claims and releases Ethereum USDJ after a future QUB burn proof verifier is configured.
/// @dev No owner withdrawal power over USDJ. The QUB proof verifier can be configured once.
contract USDJBridgeGateway {
    using BridgeSafeToken for IUSDJBridgeToken;

    uint256 public constant TOLL_BPS = 100; // 1% QUB-side protocol toll.

    IUSDJBridgeToken public immutable usdj;
    address public owner;
    address public pendingOwner;
    address public qubVerifier;
    bool public qubVerifierConfigured;
    bool public paused;
    uint256 public nextNonce;
    uint256 public lockedForQub;

    mapping(bytes32 => bool) public consumedQubBurns;

    event OwnershipTransferStarted(address indexed previousOwner, address indexed newOwner);
    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);
    event Paused(address indexed by);
    event Unpaused(address indexed by);
    event QubVerifierConfigured(address indexed verifier);
    event EthToQubLocked(uint256 indexed nonce, address indexed sender, string qubRecipient, uint256 grossAmount, uint256 tollAmount, uint256 netAmount);
    event QubToEthReleased(bytes32 indexed burnId, address indexed receiver, uint256 releaseAmount);

    error ZeroAddress();
    error NotOwner();
    error NotPendingOwner();
    error PausedError();
    error InvalidAmount();
    error InvalidRecipient();
    error VerifierAlreadyConfigured();
    error VerifierNotConfigured();
    error InvalidProof();
    error BurnAlreadyConsumed();
    error InsufficientLockedLiquidity();
    error Reentrant();

    uint256 private locked = 1;

    modifier onlyOwner() { if (msg.sender != owner) revert NotOwner(); _; }
    modifier whenNotPaused() { if (paused) revert PausedError(); _; }
    modifier nonReentrant() { if (locked != 1) revert Reentrant(); locked = 2; _; locked = 1; }

    constructor(address usdj_, address owner_) {
        if (usdj_ == address(0) || owner_ == address(0)) revert ZeroAddress();
        usdj = IUSDJBridgeToken(usdj_);
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

    function configureQubVerifier(address verifier) external onlyOwner {
        if (qubVerifierConfigured) revert VerifierAlreadyConfigured();
        if (verifier == address(0)) revert ZeroAddress();
        qubVerifierConfigured = true;
        qubVerifier = verifier;
        emit QubVerifierConfigured(verifier);
    }

    function pause() external onlyOwner { paused = true; emit Paused(msg.sender); }
    function unpause() external onlyOwner { paused = false; emit Unpaused(msg.sender); }

    /// @notice Lock Ethereum USDJ for a future QUB-chain claim. QUB side should mint netAmount to the recipient and tollAmount to the QUB protocol address.
    function bridgeToQub(uint256 grossAmount, string calldata qubRecipient) external nonReentrant whenNotPaused returns (uint256 nonce) {
        if (grossAmount == 0) revert InvalidAmount();
        bytes calldata r = bytes(qubRecipient);
        if (r.length < 8 || r.length > 96) revert InvalidRecipient();
        uint256 toll = tollFor(grossAmount);
        uint256 net = grossAmount - toll;
        usdj.safeTransferFrom(msg.sender, address(this), grossAmount);
        lockedForQub += grossAmount;
        nonce = nextNonce++;
        emit EthToQubLocked(nonce, msg.sender, qubRecipient, grossAmount, toll, net);
    }

    /// @notice Release Ethereum USDJ after a future trustless QUB burn proof verifier validates the QUB-side burn/debit.
    function releaseFromQub(bytes32 burnId, address receiver, uint256 releaseAmount, bytes calldata proof) external nonReentrant whenNotPaused {
        if (!qubVerifierConfigured) revert VerifierNotConfigured();
        if (receiver == address(0)) revert ZeroAddress();
        if (releaseAmount == 0) revert InvalidAmount();
        if (consumedQubBurns[burnId]) revert BurnAlreadyConsumed();
        if (lockedForQub < releaseAmount || usdj.balanceOf(address(this)) < releaseAmount) revert InsufficientLockedLiquidity();
        if (!IQubUsdJBurnProofVerifier(qubVerifier).verifyUsdJExit(burnId, receiver, releaseAmount, proof)) revert InvalidProof();
        consumedQubBurns[burnId] = true;
        lockedForQub -= releaseAmount;
        usdj.safeTransfer(receiver, releaseAmount);
        emit QubToEthReleased(burnId, receiver, releaseAmount);
    }

    function tollFor(uint256 amount) public pure returns (uint256) {
        return (amount * TOLL_BPS) / 10_000;
    }
}
