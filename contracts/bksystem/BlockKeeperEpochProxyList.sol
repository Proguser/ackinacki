// SPDX-License-Identifier: GPL-3.0-or-later
/*
 * GOSH contracts
 *
 * Copyright (C) 2022 Serhii Horielyshev, GOSH pubkey 0xd060e0375b470815ea99d6bb2890a2a726c5b0579b83c742f5bb70e10a771a04
 */
pragma gosh-solidity >=0.76.1;
pragma ignoreIntOverflow;
pragma AbiHeader expire;
pragma AbiHeader pubkey;

import "./modifiers/modifiers.sol";
import "./libraries/BlockKeeperLib.sol";
import "./BlockKeeperContractRoot.sol";
import "./AckiNackiBlockKeeperNodeWallet.sol";
import "./BlockKeeperCoolerContract.sol";
import "./BlockKeeperEpochContract.sol";
import "./BlockKeeperEpochProxyList.sol";

contract BlockKeeperEpochProxyList is Modifiers {
    string constant version = "1.0.0";
    mapping(uint8 => TvmCell) _code;
    mapping(uint8 => string) _ProxyList;

    optional(address) _owner;
    uint256 static _owner_pubkey;
    optional(uint256) _service_key;
    address _root;

    uint256 _walletId;
    bool status = false;

    constructor (
        mapping(uint8 => TvmCell) code,
        uint64 seqNoStart,
        optional(uint256) service_key,
        uint256 walletId
    ) {
        _code = code;
        TvmCell data = abi.codeSalt(tvm.code()).get();
        (string lib, uint256 hashwalletsalt, uint256 hashepochsalt, uint256 hashpreepochsalt, address root) = abi.decode(data, (string, uint256, uint256, uint256, address));
        require(BlockKeeperLib.versionLib == lib, ERR_SENDER_NO_ALLOWED);
        _root = root;
        _walletId = walletId;
        TvmBuilder b;
        b.store(_code[m_AckiNackiBlockKeeperNodeWalletCode]);
        uint256 hashwallet = tvm.hash(b.toCell());
        delete b;
        require(hashwallet == hashwalletsalt, ERR_SENDER_NO_ALLOWED);
        b.store(_code[m_BlockKeeperEpochCode]);
        uint256 hashepoch = tvm.hash(b.toCell());
        delete b;
        require(hashepoch == hashepochsalt, ERR_SENDER_NO_ALLOWED);
        b.store(_code[m_BlockKeeperPreEpochCode]);
        uint256 hashpreepoch = tvm.hash(b.toCell());
        delete b;
        require(hashpreepoch == hashpreepochsalt, ERR_SENDER_NO_ALLOWED);
        require(msg.sender == BlockKeeperLib.calculateBlockKeeperPreEpochAddress(_code[m_BlockKeeperPreEpochCode], _code[m_AckiNackiBlockKeeperNodeWalletCode], _root, _owner_pubkey, seqNoStart), ERR_SENDER_NO_ALLOWED);
        _service_key = service_key;
    }

    function getMoney() private pure {
        if (address(this).balance > FEE_DEPLOY_BLOCK_KEEPER_PROXY_LIST) { return; }
        gosh.mintshell(FEE_DEPLOY_BLOCK_KEEPER_PROXY_LIST);
    }

    function destroy(uint64 seqNoStart) public senderIs(BlockKeeperLib.calculateBlockKeeperEpochAddress(_code[m_BlockKeeperEpochCode], _code[m_AckiNackiBlockKeeperNodeWalletCode], _code[m_BlockKeeperPreEpochCode], _root, _owner_pubkey, seqNoStart)) accept {
        selfdestruct(_root);
    }

    function setOwner(optional(address) owner) public onlyOwnerCombine(_owner, _service_key, _owner_pubkey) accept {
        getMoney();
        _owner = owner;
    } 

    function addProxyList(mapping(uint8 => string) data) public onlyOwnerCombine(_owner, _service_key, _owner_pubkey) accept {
        require(status == false, ERR_NOT_READY);
        getMoney();
        status = true;
        this.iterateProxyList{value: 0.1 vmshell}(data, data.min(), true);
    }

    function deleteProxyList(mapping(uint8 => string) data) public onlyOwnerCombine(_owner, _service_key, _owner_pubkey) {
        require(status == false, ERR_NOT_READY);
        getMoney();
        status = true;
        this.iterateProxyList{value: 0.1 vmshell}(data, data.min(), false);   
    }

    function iterateProxyList(mapping(uint8 => string) data, optional(uint8, string) member, bool action) public senderIs(address(this)) accept {
        getMoney();
        if (member.hasValue() == false) {
            status = false;
            return;
        }
        (uint8 key, string value) = member.get();
        if (action) {
            _ProxyList[key] = value;
        } else {
            delete _ProxyList[key];
        }
        this.iterateProxyList{value: 0.1 vmshell}(data, data.next(key), true);   
    }


    
    //Fallback/Receive
    receive() external {
    }

    //Getters
    function getDetails() external view returns(
        uint256 pubkey,
        address root, 
        mapping(uint8 => string) ProxyList,
        optional(address) owner,
        optional(uint256) service_key) 
    {
        return (_owner_pubkey, _root, _ProxyList, _owner, _service_key);
    }

    function getVersion() external pure returns(string, string) {
        return (version, "BlockKeeperEpochProxyList");
    }  
}
