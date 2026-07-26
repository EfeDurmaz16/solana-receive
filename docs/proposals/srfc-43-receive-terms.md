# sRFC 43: Receive Terms and Held Delivery

**Status:** Draft
**Author:** Efe Baran Durmaz (@EfeDurmaz16)
**Reference implementation:** https://github.com/EfeDurmaz16/solana-receive
**Neighbours:** sRFC 37 (Token ACL, issuer side), sRFC 40 (Vault Standard), sRFC 42 (Silent Payments)

The key words MUST, MUST NOT, REQUIRED, SHOULD, SHOULD NOT and MAY are to be interpreted as
described in RFC 2119.

---

## Summary

A receiving account on Solana accepts everything. This sRFC standardises the opposite: a receiver
publishes **receive terms** for a mint, a conforming sender resolves those terms before paying, and
value that does not satisfy them is routed into a **guard** vault with a claimable **receipt**
rather than credited to the receiver or bounced back to the sender.

This is an application standard. It runs on unmodified Token-2022 and unmodified SPL Token, requires
no core change, and degrades to today's behaviour when either party does not participate.

---

## 1. Motivation

### 1.1 The concrete case

A payment service provider settles merchant balances in USDC. Its settlement account is public,
because merchants and their customers need to pay into it. Today that account credits:

- tokens the PSP does not carry and cannot price,
- transfers from counterparties its compliance policy forbids it from holding,
- dust below the amount at which reconciliation is economic.

Every one of those becomes an accounting entry the PSP must handle after the fact, and some of them
are liabilities the moment they land. The PSP cannot refuse them, because Solana has no way for a
destination to say what it accepts.

The same shape appears for an exchange's deposit address, a DAO treasury, a merchant's point of
sale, and any on-chain account whose address is published so strangers can pay it.

### 1.2 Why bouncing is not the answer

The obvious fix is to make a non-matching transfer fail. That is what the existing tools do, and it
is the wrong outcome for a payment rail:

- A sender who pays in good faith and is bounced does not know why, and retries.
- A payroll or invoice batch that reverts on one bad line loses the whole batch.
- A merchant who wants to review a borderline payment has no way to hold it.

A payment rail needs a third outcome between "accepted" and "rejected": **received but not
credited**, with a defined recovery path. That is what this proposal specifies.

### 1.3 Why the existing pieces do not cover it

| Mechanism | Who decides | Failure mode |
| --- | --- | --- |
| Token-2022 Transfer Hook | Mint issuer | Reverts. Receives de-escalated read-only accounts, so it can only approve or reject. |
| Token-2022 Default Account State | Mint issuer | Freezes the whole account, not per-transfer. The receiver has no say. |
| sRFC 37 Token ACL | Mint issuer | Standardises issuer allow and block lists. Its three stated use cases all begin "As an issuer". |
| Vault `deposit()` (sRFC 40) | Receiving app | An explicit instruction. A wallet-to-ATA transfer never enters it. |

None of these lets the **destination** state terms, and none produces a non-reverting outcome.

---

## 2. Relationship to neighbouring standards

**sRFC 37 (Token ACL) is the issuer side of the same question and composes with this one.** An issuer
ACL is a *validity* condition on a transfer; receive terms are a *routing* decision on an
already-valid transfer. Issuer strictly dominates: if the issuer's gate has not thawed the
destination, the transfer fails and terms are never evaluated.

Implementers combining the two MUST note that under `DefaultAccountState = Frozen` a newly created
guard vault is frozen, and held delivery cannot function until the issuer's gate program thaws the
guard's owner. That owner is a program-derived address, not a KYC-able entity. Reconciling this
requires a decision on sRFC 37's side and is called out here as a dependency rather than assumed
away.

**sRFC 40 (Vault Standard) already has a three-state deposit** (`RequestDeposit`, `Pending`,
`Claimable`, `Rejected`, `Claim`), including partial claims, which this proposal does not have. The
difference is not the state machine. It is custody and reach:

| | sRFC 40 vault | This proposal |
| --- | --- | --- |
| Who holds escrowed value | Vault authority, who also has `WithdrawAssets` | A program-derived address with no keypair |
| Adversary model | Depositor trusts the vault operator | Receiver is modelled as the adversary |
| Entry point | A per-application instruction | A standard account list any sender can build |

An earlier draft of this work argued that vaults are a cooperative rail that ordinary ATA transfers
never enter. That argument is withdrawn: this proposal is also cooperative, because a sender must
resolve terms and pass additional accounts. The honest differentiator is custody, stated above.

<img width="932" alt="Custody comparison: a vault authority can withdraw escrowed assets, a program-derived guard owner cannot be signed for at all" src="https://raw.githubusercontent.com/EfeDurmaz16/solana-receive/main/docs/assets/custody-comparison.svg" />

**sRFC 42 (Silent Payments) is orthogonal and shares one piece of machinery.** Silent Payments
changes *where* a payment lands so an observer cannot link it; this proposal changes *whether* a
payment counts as delivered. Both, however, require a sender to resolve something the receiver
published before constructing the transfer. Solana has no convention for that resolution step, and
both proposals are inventing one independently. A shared receiver-published-parameters convention
would serve both and is proposed as an open question in section 10.

---

## 3. Overview

<img width="932" alt="Outcome flow: a sender resolves receive terms, and a refusal routes to a guard instead of reverting" src="https://raw.githubusercontent.com/EfeDurmaz16/solana-receive/main/docs/assets/outcome-flow.svg" />

Three outcomes, and exactly one of them is new:

| Outcome | Meaning | Receiver balance | Recoverable |
| --- | --- | --- | --- |
| `credited` | Terms accept | Increases | n/a |
| `held` | Terms reject, sender permitted a hold | Unchanged | Yes, by claim or expiry |
| `failed` | Ordinary token failure, or terms reject and the sender forbade holds | Unchanged | n/a, nothing moved |

---

## 4. Receive terms

### 4.1 Two-level resolution

Terms resolve in two levels so that a wallet can set one default and still override a single
account. A conforming sender MUST resolve in this order and MUST stop at the first hit:

1. **Account-specific terms** at `["receive-terms", destination_token_account]`
2. **Wallet-default terms** at `["receive-terms", destination_owner, mint]`
3. **No terms.** The sender performs an ordinary transfer.

Both derivations are computable by the sender from public inputs, so resolution requires no indexer
and no registry. A sender that cannot fetch either account MUST treat the destination as having no
terms rather than failing, so that an RPC outage degrades to today's behaviour.

### 4.2 Account layout

`ReceiveTerms` is owned by the receive program.

| Offset | Size | Field | Notes |
| --- | --- | --- | --- |
| 0 | 8 | `discriminator` | `sha256("receive-terms-standard:ReceiveTerms")[0..8]` |
| 8 | 1 | `version` | `1`. An unrecognised version MUST fail closed. |
| 9 | 32 | `authority` | The only key permitted to replace these terms. |
| 41 | 32 | `mint` | `Pubkey::default()` in account-specific terms, which bind to one account. |
| 73 | 32 | `policy_program` | `Pubkey::default()` means "accept everything". |
| 105 | 8 | `receipt_bond_lamports` | Requested bond. The effective bond is section 6.2. |
| 113 | 8 | `receipt_ttl_slots` | MUST be non-zero. `0` is invalid, not a sentinel for a default. |
| 121 | 1 | `recovery_authority_mode` | `0` Originator, `1` Receiver, `2` ThirdParty. Ordering is normative. |
| 122 | 32 | `recovery_authority` | Read only when `recovery_authority_mode == 2`. |
| 154 | 6 | `_reserved` | MUST be zero. |

Total 160 bytes.

Terms are replaceable by `authority` and MUST NOT be replaceable by anyone else. Replacement takes
effect for transfers in later slots only; see section 8.3.

---

## 5. The `can_credit` interface

This is what makes the proposal a standard rather than one program's opinion. A receiver's actual
rule lives in a **policy program** of their choosing. The receive program does not interpret rules;
it dispatches.

A conforming policy program MUST implement:

**`can_credit`**

- Discriminator preimage: `receive-terms-standard:can-credit`
- Discriminator: `[0x3b, 0x59, 0xde, 0xf5, 0x15, 0xcb, 0x3b, 0xad]`
- Instruction data after the discriminator: `amount: u64`, `mint: Pubkey`, `source_owner: Pubkey`,
  `destination: Pubkey`
- Accounts, in order, **exactly six, never more**:

| # | Account | Notes |
| --- | --- | --- |
| 0 | `terms` | The `ReceiveTerms` account being evaluated |
| 1 | `mint` | |
| 2 | `destination` | The destination token account |
| 3 | `source_owner` | |
| 4 | `flag` | Proof the call came from the receive program, below |
| 5 | `policy_state` | The policy program's own state, at `["policy-state", terms]` under the policy program |

**The account list is fixed and so are the seeds.** There is no extra-account-metas resolution and
no variable tail. This is a deliberate departure from the transfer-hook pattern, and it responds to
feedback in sRFC 37's own thread, where an implementer who had shipped both sRFC 37 and transfer
hooks wrote that extra account metas are "extremely painful to debug", that an incorrect account
gives no signal about whether the pubkey or the seed was wrong, and that they "would not be opposed
to a more opinionated standard that fixes the number of accounts and possibly even the PDA seeds".

The cost of that choice is stated plainly: a policy program that needs more than one state account
cannot have one in v0. It must pack its state under `["policy-state", terms]`, or keep the extra
state off chain and commit to it by hash. In exchange, every `can_credit` call has an identical,
statically known account list, a sender can build the transaction without querying the policy
program, and a failure names one of six accounts rather than an unbounded set.

Return semantics: `Ok(())` means credit. Any error means do not credit. A policy program MUST NOT
assume that an error causes the outer transfer to fail; the receive program converts a refusal into
`held` or `failed` per section 6.

**The flag account.** The receive program creates a zero-lamport account owned by itself, holding
one byte set to `1`, for the duration of the call. A policy program that keeps bookkeeping MUST
verify both that the flag account is owned by the receive program and that its data is `[1]`, so it
cannot be driven by a caller impersonating the interface.

**De-escalation.** The receive program MUST strip signer and writable privileges from every account
it forwards to the policy program. A policy program therefore cannot move funds, and a malicious
policy program cannot escalate a transfer it was merely asked to judge.

**Evaluation is not a pure predicate.** A conforming implementation MUST call `can_credit` exactly
once per transfer and MUST NOT short-circuit on any locally cached condition. Two implementations
that disagree about when to skip the call will disagree about outcomes.

---

## 6. Held delivery

### 6.1 The guard

A guard is a token account whose SPL owner is a program-derived address, sharded by
`(destination_owner, mint)`. Custody remains at wallet level even though terms resolve at two
levels, because custody is a recovery unit and fragmenting it by token account would multiply the
accounts a receiver must sweep without changing who may sweep them.

No keypair exists for a program-derived address, and the System Program requires a target to sign
for `create_account`, `allocate` and `assign`. Therefore only the receive program can ever give a
guard any data, and the receiver who refused the payment cannot spend it.

A guard MUST be refused as the destination of any transfer or mint, and as the source of any
transfer, on every credit path. Value reaching a guard without an accompanying receipt would be
unrecoverable.

### 6.2 The bond and the receipt

Every hold writes a `Receipt` account, funded by a bond. The effective bond is
`max(terms.receipt_bond_lamports, rent_exempt(304))`, where 304 is the receipt size in bytes fixed
by section 6.4, and a conforming implementation MUST compare a sender's declared ceiling against
this rent-floored value, not against the raw terms field.

The bond exists so that a receiver cannot make holding expensive for the sender's counterparty, and
so that the account has a funder with a claim on its rent. On close, the recorded `bond_lamports`
MUST be refunded to the recorded `bond_payer`. Any surplus lamports at the receipt address MUST NOT
be treated as bond.

### 6.3 Sender-declared limits

A sender MUST be able to bound what a receiver can do with its money. `HeldLimits` carries:

| Field | Meaning |
| --- | --- |
| `never_hold: bool` | If set, a refusal MUST produce `failed`, never `held`. |
| `max_bond_lamports: u64` | Refuse the hold if the effective bond exceeds this. |
| `max_ttl_slots: u64` | Refuse the hold if the terms' TTL exceeds this. |
| `max_recovery_mode: u8` | Refuse the hold if `terms.recovery_authority_mode` exceeds this. |

`never_hold` is an explicit flag. An earlier design expressed it as `max_bond = max_ttl =
max_recovery = 0` and relied on the effective bond being non-zero because rent is non-zero. That
guarantee is emergent rather than stated, and a conforming implementation on a different fee or rent
regime would silently turn the strongest sender guarantee into a hold. A standard MUST NOT encode a
guarantee as an accident.

`max_recovery_mode` is the field that matters most and is the least obvious. Capping cost alone
still leaves the rejecting party discretion over the money. A sender that sets
`max_recovery_mode = 0` accepts holds only when it remains the claim authority.

### 6.4 Account layouts

A standard that does not publish its account layouts is not implementable by anyone who has not
read the reference source. Both accounts below are `repr(C)`, little-endian, fixed size, and owned
by the receive program. Reserved bytes MUST be zero on write and MUST be ignored on read. An
unrecognised `version` MUST fail closed rather than being reinterpreted.

**`Receipt`, 304 bytes.** Discriminator `0x5245435652435054`, the ASCII bytes `RECVRCPT`.
PDA seeds: `["receipt", destination_owner, mint, source_owner, unique_nonce]`.

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 8 | `discriminator` |
| 8 | 8 | `amount` |
| 16 | 32 | `mint` |
| 48 | 32 | `source_token_account` (the pinned refund target, section 8.3) |
| 80 | 32 | `source_owner` |
| 112 | 32 | `destination_token_account` |
| 144 | 32 | `receiver_owner` |
| 176 | 1 | `recovery_authority_mode` (`0` Originator, `1` Receiver, `2` ThirdParty) |
| 177 | 1 | `status` (`1` Open, `2` Closed) |
| 178 | 1 | `version` (`1`) |
| 179 | 5 | reserved |
| 184 | 32 | `recovery_authority` |
| 216 | 8 | `created_slot` |
| 224 | 8 | `expires_slot` |
| 232 | 8 | `bond_lamports` |
| 240 | 32 | `bond_payer` |
| 272 | 32 | `unique_nonce` |

`unique_nonce` is chosen by the sender and exists so that one sender can hold more than one
concurrent receipt against the same `(receiver, mint)` without a PDA collision. Senders SHOULD draw
it from a cryptographic random source. A nonce whose receipt account still holds data MUST be
rejected rather than reused.

A receipt is authenticated by re-deriving its own address from the fields it claims about itself.
A conforming implementation MUST perform that re-derivation, so that a fabricated receipt at a
different address cannot be settled even if its bytes are well formed.

**`GuardState`, 128 bytes.** Discriminator `0x5245435647554152`, the ASCII bytes `RECVGUAR`.
PDA seeds: `["guard-state", destination_owner, mint]`. The guard token account it describes is at
`["guard", destination_owner, mint]`.

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 8 | `discriminator` |
| 8 | 1 | `version` (`1`) |
| 9 | 7 | reserved |
| 16 | 32 | `receiver` (the destination's owner, not the token account) |
| 48 | 32 | `mint` |
| 80 | 32 | `guard_token_account` |
| 112 | 8 | `open_receipts` |
| 120 | 8 | `held_amount` |

`held_amount` is the sum of the amounts of every open receipt in the shard. A conforming
implementation MUST assert `guard_token.amount >= held_amount` after every mutation and MUST fail
closed when it does not hold, rather than relying on the invariant holding by construction. A guard
vault holding more than `held_amount` is valid; the surplus is not claimable and MUST NOT be paid
out against any receipt.

<img width="932" alt="Accounts and PDA seeds: two levels of receive terms, an external policy program, and the guard, guard state and receipt PDAs" src="https://raw.githubusercontent.com/EfeDurmaz16/solana-receive/main/docs/assets/account-map.svg" />

Note that `receiver` throughout is the destination token account's **owner**, never the token
account address. Two token accounts owned by the same wallet for the same mint therefore share one
guard shard and one receipt namespace, even when they carry different account-specific terms under
section 4.1. This is deliberate: terms are policy and belong to the account, custody is a recovery
unit and belongs to the wallet.

---

## 7. Determining the outcome

This section is normative and supersedes any convenience the reference implementation offers.

**An off-chain consumer MUST determine the outcome from the destination token account's balance
delta.** Return data and program logs MUST NOT be treated as authoritative.

<img width="932" alt="Signal channels: return data is forgeable, logs are droppable and forgeable, only the destination balance delta is authoritative" src="https://raw.githubusercontent.com/EfeDurmaz16/solana-receive/main/docs/assets/signal-channels.svg" />

Both weaker channels are defeatable by a hostile payer, who is precisely the adversary this standard
exists to defend against:

- **Return data is transaction-scoped, not instruction-scoped.** A payer can append any instruction
  after the transfer and overwrite it. `meta.returnData` on a landed transaction reflects the last
  instruction that set it, not the transfer.
- **Logs are droppable and forgeable.** The runtime's per-transaction log buffer is bounded, so a
  payer who pads the transaction can push the outcome line out of it. Any program in the same
  transaction can also emit an identical line.

Return data remains authoritative in exactly one case: a program that CPIs the transfer and calls
`get_return_data()` immediately, before any further instruction runs. In that case the transfer
sets a single byte, `0` for `credited` and `1` for `held`, and `failed` sets nothing because the
instruction returns an error. Client libraries that decode return data MUST require the program id
that the RPC returns alongside it and MUST reject data attributed to any other program. A decoder
that accepts a bare byte array will accept a value planted by any program in the transaction.

A held transfer is observable independently of both channels, because it writes a `Receipt` account
and increments the guard's `held_amount`. Indexers SHOULD watch those accounts. Section 8.2
guarantees they are observable for at least one slot.

---

## 8. Recovery

### 8.1 Claim

`ClaimReceipt` is callable by the recovery authority named in the terms at the time of the hold. The
claim destination MUST be checked against its own receive terms; crediting a claim destination
without evaluating its terms would let any party bypass the standard by routing through a hold.

### 8.2 The creation-slot gate

`ClaimReceipt` MUST fail when `clock.slot <= receipt.created_slot`.

The purpose is not rate limiting. It is to guarantee that every held receipt exists on chain across
at least one slot boundary, so that an indexer scanning accounts cannot miss a hold that was created
and unwound inside a single transaction. Without this gate, a payer can hold and immediately reclaim
in one transaction, leaving no residual account after the receipt is reaped.

This gate does not by itself make the outcome signal trustworthy. Section 7 does. The gate makes the
durable channel complete.

### 8.3 Expiry

After `created_slot + ttl_slots`, anyone MAY close the receipt. Tokens MUST be returned to the
account recorded as `receipt.source_token_account`, not to an arbitrary account whose owner field
matches. Token accounts can be created with an arbitrary owner field and no signature from that
owner, so an unpinned refund target lets a third party redirect refunds into an account the sender
never created and cannot close.

Lamports MUST be refunded to `receipt.bond_payer`.

Terms replacement MUST NOT affect a receipt already written. A receipt carries its own recovery
authority, bond payer and expiry, and is settled against those recorded values.

---

## 9. Conformance

**A conforming sender** MUST resolve terms per section 4.1, MUST declare `HeldLimits`, MUST NOT
assume a landed transaction credited the destination, and SHOULD offer the payer a preview of the
outcome before signing.

### 9.1 What happens when the sender does not participate

This standard depends on sender-side cooperation, and that dependency deserves to be stated rather
than discovered. sRFC 37's thread is currently living the same problem from the other side: an
implementer reported in July 2026 that Phantom creates the ATA but does not append
`ThawPermissionless`, so the account stays frozen and the transfer fails with `0x11`, and Phantom
confirmed the flow is not supported today.

The two standards fail in opposite directions, and neither direction is strictly better:

| | Non-participating sender |
| --- | --- |
| sRFC 37 | Fails closed. The payment reverts with `AccountFrozen`. The receiver's rule is enforced, but a payer who did nothing wrong cannot pay. |
| This proposal | Degrades open. The transfer credits the destination as it does today. The payer is never blocked, but the receiver's terms are not applied. |

Degrading open is the right default for a rail whose whole premise is that a refusal should not
punish the sender, and it means adoption can begin with a single cooperating PSP rather than
requiring every wallet first. It also means **terms are not a security boundary against a
non-participating sender.** A receiver who needs enforcement against arbitrary senders wants an
issuer-side control such as sRFC 37, not this. A receiver who wants to classify and quarantine what
its own counterparties send wants this.

**A conforming receiver** MUST NOT treat `held` value as received, and MUST be able to enumerate its
own guard shard.

**A conforming indexer** MUST determine outcomes per section 7. An indexer that books a payment from
a successful transaction status alone is not conforming, and will book payments that did not arrive.

**A conforming policy program** MUST implement `can_credit` per section 5, MUST verify the flag
account, and MUST NOT assume its refusal reverts the transfer.

Sections 4.2, 5 and 6.4 are the normative source for encoding. Byte-level test vectors covering
every instruction and account layout are published alongside the reference implementation and serve
as the conformance suite against which a second implementation can check itself.

---

## 10. Open questions

1. **A shared resolution convention.** sRFC 42 needs senders to resolve a published meta-address;
   this proposal needs senders to resolve published terms. Should Solana standardise
   receiver-published parameter resolution once, rather than twice?
2. **sRFC 37 interaction.** Held delivery under `DefaultAccountState = Frozen` requires the issuer's
   gate to thaw an address that is a program-derived address rather than an entity. Does that belong
   in sRFC 37 as an exemption, or here as a documented incompatibility?
3. **Partial claims.** sRFC 40 supports them. This proposal does not. Is a partial claim meaningful
   when the held amount corresponds to one refused payment?
4. **Contention.** Every hold against one `(owner, mint)` writes the same guard shard. A high-volume
   receiver serialises inbound refusals. Is per-shard sharding by a sender-derived index worth the
   added resolution complexity?

---

## 11. Non-claims

This proposal does not modify Token-2022, does not intercept transfers a sender did not construct
against this standard, and has not been audited or deployed to mainnet. Where a sender does not
participate, behaviour is identical to Solana today.
