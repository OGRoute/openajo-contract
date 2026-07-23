---
description: >-
  The Soroban smart contracts behind OpenAjo — rotating savings circles on
  Stellar, enforced by code instead of a collector you have to trust.
---

# Introduction

This is the contract-level documentation for **openajo-contract**, the on-chain
core of [OpenAjo](https://ogroute.gitbook.io/ogroute-docs) — rotating savings
circles (_ajo_, _esusu_, _adashe_) on Stellar.

A rotating savings circle is simple: a group agrees to contribute a fixed amount
each period, and each period the whole pot pays out to one member in turn, until
everyone has been paid once. It is one of the most widely used savings
instruments in the world, and it has exactly two weaknesses — both of them trust
problems. The person holding the money can take it, and members can stop paying
after they have received their turn.

These contracts remove both. No one holds the money — the contract escrows it and
pays out on a fixed rotation. And missed contributions are covered by a deposit
the defaulter posted up front, so the members late in the rotation are protected
by collateral rather than by hope.

## Two contracts

| Contract | Responsibility |
| --- | --- |
| **`circle`** | The full lifecycle for many circles: create, join, contribute, settle, complete. Holds all escrowed funds. |
| **`reputation`** | A permanent, cross-circle record of completions and defaults per address. Written only by contracts on an allow-list. |

They are deployed together. `circle` calls `reputation` to record how each
membership ended, and `reputation` accepts those reports only because `circle`
was added to its reporter allow-list at deploy time.

## Deployed on Stellar testnet

```
circle       CCLVOHGHDH32GWFAMCEMVHLNJSF6ENVHERYWU2OHUYWWLAOKLVR3HGKS
reputation   CDXPH2PYUTRW7GV57X6CJH3E3JOPROSC23NXPMAXOO3EOBI5UTCB2GTQ
```

* [`circle` on Stellar Expert](https://stellar.expert/explorer/testnet/contract/CCLVOHGHDH32GWFAMCEMVHLNJSF6ENVHERYWU2OHUYWWLAOKLVR3HGKS)
* [`reputation` on Stellar Expert](https://stellar.expert/explorer/testnet/contract/CDXPH2PYUTRW7GV57X6CJH3E3JOPROSC23NXPMAXOO3EOBI5UTCB2GTQ)

## The idea worth understanding first

The deposit **does not** fully collateralize a member, and that is deliberate. A
flat bond smaller than the total a member could owe over a whole circle is enough
to *price* default — a defaulter loses their bond — without locking up as much
capital as the circle is worth. What stops repeat defaulting across circles is
not a bigger bond, it is [reputation](design/reputation.md): the default is
recorded permanently and every future circle can see it.

This is the single most important design decision in the protocol, and it is easy
to "fix" by mistake. The `deposit` field carries a comment in the source saying
so. Read [Deposits, slashing & default](design/deposits-and-default.md) before
proposing any change there.

## Where to go next

* How a circle moves through its states → [The circle lifecycle](design/lifecycle.md)
* How the pot is collected and paid → [Settlement](design/settlement.md)
* Function-by-function → [`circle` reference](reference/circle.md)
* Building and deploying → [Build and test](building/build-and-test.md)

{% hint style="danger" %}
OpenAjo is **unaudited and testnet-only**. These contracts hold funds and have
not been through a security audit. Do not deploy with mainnet value.
{% endhint %}
