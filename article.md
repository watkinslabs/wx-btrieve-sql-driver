# Time as a Dependency: Rebuilding a Btrieve World Without Breaking It

Most modernization stories are told as victory laps over old technology: we replaced the obsolete thing with the new thing and moved on.  
That is not this story.

This is a story about software from another era—DOS software born in the 1980s, built around Btrieve, storing business reality in flat files on disks that were sometimes booted from floppies. It still mattered. It still ran the business. And it still carried decades of assumptions, workflows, and trust.

We didn’t “rip and replace” it. We translated it.

## The philosophical frame: time is a dependency

In modern engineering, we talk about runtime dependencies, network dependencies, library dependencies. In legacy engineering, there is another one: **time**.

Old systems are not just old code. They are accumulated agreements:

- agreements about record layouts and index behavior
- agreements about error codes and edge cases
- agreements between users and software that were never written down because “that’s just how it works”

When software survives for decades, those agreements become infrastructure. Change them carelessly and you don’t just introduce bugs—you break institutional memory.

So the challenge was not merely technical. It was temporal: how do you move a system across generations of architecture without breaking the agreements it made with the past?

## The technical crossing: from DOS Btrieve to SQL Server

At a high level, we built a bridge with two commitments:

1. Preserve Btrieve-facing behavior at the call boundary.
2. Replace the storage engine underneath with Microsoft SQL Server.

That meant keeping the legacy interface alive while changing almost everything behind it.

The runtime is split into layers:

- a Windows DLL surface that exposes the Btrieve-compatible entry points
- DOS/NTVDM glue that intercepts and forwards the expected interrupt-driven patterns
- a portable core that implements operation semantics and record handling
- a metadata/config layer (SQLite) that maps legacy table schemas and flags
- an adapter that turns Btrieve-style operations into SQL Server interactions

The important detail is philosophical as much as technical: we did not ask the DOS application to become modern. We built a modern substrate that could faithfully speak its language.

## Why a shim is not a hack

“Shim” can sound temporary. In this case, the shim is the architecture.

A good compatibility layer is not duct tape; it is a boundary of respect. It says:

- legacy behavior is a contract, not an inconvenience
- modern infrastructure is an implementation detail, not a moral superiority
- migration succeeds when users do not have to relearn the world overnight

By keeping the old operational semantics intact while moving persistence to SQL Server, we gained durability, manageability, and server-grade operations without demanding that decades-old software rewrite itself.

## DOS device driver, modern consequences

There is something almost poetic about this: a DOS device-driver pathway, revived inside a modern host, now participating in a data system that can live in contemporary server infrastructure.

The stack spans eras:

- 16-bit expectations at the boundary
- 32-bit Windows runtime behavior in the middle
- modern relational storage behind the curtain

That is not nostalgia engineering. That is systems continuity engineering.

## What actually got migrated

People say “we migrated data.”  
We did migrate data. But the harder migration was trust.

Trust that “Next” still means next in the same way.  
Trust that key lookups and status codes still behave as muscle memory expects.  
Trust that the business process encoded over decades still has a stable floor.

That is why the real unit of migration is not rows. It is expectations.

## A better modernization story

The usual modernization narrative is replacement: old out, new in.

This one is translation across time:

- keep the contract
- change the substrate
- preserve behavior
- expand operational possibility

In that sense, the deepest dependency in this project was not a crate or a driver API.  
It was time itself.

And the result is simple to say, hard to do:

**We didn’t migrate data; we migrated trust.**
