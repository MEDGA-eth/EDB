// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

/// Simple bank that emits events, used to exercise multiple cheatcodes
/// interacting in a single test function.
contract Bank {
    mapping(address => uint256) public balanceOf;

    event Deposit(address indexed user, uint256 amount);
    event Withdraw(address indexed user, uint256 amount);

    function deposit() external payable {
        balanceOf[msg.sender] += msg.value;
        emit Deposit(msg.sender, msg.value);
    }

    function withdraw(uint256 amount) external {
        require(balanceOf[msg.sender] >= amount, "insufficient");
        balanceOf[msg.sender] -= amount;
        emit Withdraw(msg.sender, amount);
        (bool ok, ) = msg.sender.call{value: amount}("");
        require(ok, "transfer");
    }
}

/// Interaction tests: each function combines 3+ cheatcodes in a single test to
/// catch state-bleeding bugs that wouldn't surface in single-cheatcode fixtures.
contract Combo is Test {
    event Deposit(address indexed user, uint256 amount);

    /// vm.deal + vm.prank + vm.expectEmit + actual deposit
    ///
    /// Interaction: prank overrides msg.sender for the deposit call;
    /// expectEmit must match the Deposit log emitted with alice as the indexed
    /// user; deal must have funded alice so the payable call succeeds.
    function testPrankDealEmit() public {
        Bank bank = new Bank();
        address alice = address(0xA11CE);

        vm.deal(alice, 5 ether);

        // Soft-match: any Deposit log from any emitter satisfies this expectation
        // (EDB v1 semantics; topic[0] = event sig is present, check topic[1] = user).
        vm.expectEmit(true, false, false, false);
        emit Deposit(alice, 1 ether);

        vm.prank(alice);
        bank.deposit{value: 1 ether}();

        require(bank.balanceOf(alice) == 1 ether, "balance wrong after prank+deposit");
    }

    /// vm.warp + vm.mockCall + vm.expectCall
    ///
    /// Interaction: warp sets block.timestamp; mockCall intercepts balanceOf;
    /// expectCall verifies the call to balanceOf was dispatched.
    function testWarpMockExpectCall() public {
        Bank bank = new Bank();
        vm.warp(20240101);
        require(block.timestamp == 20240101, "warp did not apply");

        // Mock bank.balanceOf(address(this)) to return 999 even though nothing
        // was deposited (tests that mockCall + expectCall accounting are consistent).
        vm.mockCall(
            address(bank),
            abi.encodeWithSelector(bank.balanceOf.selector, address(this)),
            abi.encode(uint256(999))
        );

        // expectCall: the next call to balanceOf(address(this)) must happen.
        vm.expectCall(
            address(bank),
            abi.encodeWithSelector(bank.balanceOf.selector, address(this))
        );

        uint256 bal = bank.balanceOf(address(this));
        require(bal == 999, "mockCall did not intercept");
    }

    /// vm.deal + vm.startPrank + vm.expectRevert + vm.stopPrank
    ///
    /// Interaction: startPrank sets persistent caller override; a legitimate
    /// deposit works under prank; expectRevert intercepts the revert from an
    /// over-limit withdraw; stopPrank clears prank state so subsequent calls are
    /// not affected.  After stopPrank the balance must still reflect the deposit.
    function testStartPrankExpectRevertStopPrank() public {
        Bank bank = new Bank();
        address bob = address(0xB0B);
        vm.deal(bob, 1 ether);

        vm.startPrank(bob);
        bank.deposit{value: 1 ether}();

        // Bob only deposited 1 ether; withdrawing 2 must revert.
        // Use bare expectRevert() so EDB's v1 string-vs-Error(string) encoding
        // difference doesn't matter — we only care that the call reverts at all.
        vm.expectRevert();
        bank.withdraw(2 ether);

        vm.stopPrank();

        // After stopPrank bob's balance must be unchanged (the failed withdraw
        // was rolled back by expectRevert).
        require(bank.balanceOf(bob) == 1 ether, "bob balance wrong after failed withdraw");
    }
}
