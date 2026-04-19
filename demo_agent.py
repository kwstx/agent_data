import asyncio
import logging
from sdk import VNodeClient, SecurityError
from rich.console import Console
from rich.table import Table
from rich.live import Live
from rich.panel import Panel
from rich.layout import Layout
from datetime import datetime

# Setup console for rich output
console = Console()

# Silence noisy logs for cleaner demo
logging.getLogger("websockets").setLevel(logging.ERROR)
logging.getLogger("aiohttp").setLevel(logging.ERROR)

async def run_agent_demo():
    console.print(Panel(
        "[bold white]Verifiable State Layer - Protocol Evolution[/bold white]\n"
        "[dim]Permissionless onboarding, Generalized Schemas, and Cross-chain Verification.[/dim]",
        subtitle="v1.1.0-protocol",
        border_style="bright_blue"
    ))

    try:
        async with VNodeClient(base_url="http://localhost:3000") as sdk:
            
            # --- 1. Verified Query Section ---
            console.print("\n[bold cyan]▶ Requesting Generalized Verified State[/bold cyan]")
            try:
                eth_price = await sdk.get_verified_value("ETH/USD")
                console.print(Panel(
                    f"Key: [bold]ETH/USD[/bold]\n"
                    f"Value: [bold green]{eth_price}[/bold]\n"
                    f"Status: [bold green]CRYPTOGRAPHICALLY VERIFIED[/bold green]",
                    title="Verified Query Result",
                    border_style="green",
                    expand=False
                ))
            except SecurityError as e:
                console.print(f"[bold red]Security Violation:[/bold red] {e}")
            except Exception as e:
                console.print(f"[bold yellow]Wait:[/bold yellow] Is the vnode service running? ({e})")
                return

            # --- 2. Permissionless Onboarding Section ---
            console.print("\n[bold cyan]▶ Simulating Permissionless Node Onboarding[/bold cyan]")
            import aiohttp
            async with aiohttp.ClientSession() as session:
                payload = {"node_id": "agent_node_zeta", "stake": 500.0}
                async with session.post("http://localhost:3000/nodes/join", json=payload) as resp:
                    if resp.status == 200:
                        console.print(f"[bold green]✔ Node Onboarded:[/bold green] Agent node successfully staked and joined the protocol.")
                    else:
                        console.print(f"[bold red]✘ Onboarding Failed:[/bold red] {await resp.text()}")

            # --- 3. Cross-Chain Verification Section ---
            console.print("\n[bold cyan]▶ Executing Query-Verify-Act Loop (Cross-Chain)[/bold cyan]")
            # Fetch a proof and root
            async with aiohttp.ClientSession() as session:
                async with session.get("http://localhost:3000/state/ETH/USD") as resp:
                    data = await resp.json()
                    proof_payload = {
                        "key": "ETH/USD",
                        "value": data["value"],
                        "root": data["proof"]["merkle_root"],
                        "proof_path": data["proof"]["proof_path"]
                    }
                    
                    # Verify independently via the verification endpoint (simulating a smart contract/different chain)
                    async with session.post("http://localhost:3000/verify/proof", json=proof_payload) as vresp:
                        vresult = await vresp.json()
                        if vresult["valid"]:
                            console.print(f"[bold green]✔ Independent Verification Success![/bold green]")
                            console.print(f"  [dim]Resulting state is trusted manually without trusting the node provider API.[/dim]")
                        else:
                            console.print(f"[bold red]✘ Verification Failed![/bold red]")

            # --- 2. Live Stream Section ---
            console.print("\n[bold cyan]▶ Subscribing to Verified State Stream (Push Model)[/bold cyan]")
            
            table = Table(box=None, header_style="bold magenta")
            table.add_column("Epoch", justify="center")
            table.add_column("Asset", style="bright_white")
            table.add_column("Verified Value", justify="right", style="bright_green")
            table.add_column("Merkle Root (Proof Anchor)", style="dim")
            table.add_column("Quorum Status", justify="center")

            async def update_ui(snapshot):
                # This callback only receives snapshots that passed SDK internal verification
                table.rows.clear()
                for key, val in snapshot.values.items():
                    table.add_row(
                        f"#{snapshot.epoch}",
                        key,
                        f"${val:.4f}",
                        f"{snapshot.merkle_root[:16]}...",
                        f"[bold green]✓ {len(snapshot.signatures)} Nodes[/bold green]"
                    )

            # Start subscription in background
            subscribe_task = asyncio.create_task(sdk.subscribe(update_ui))

            console.print("[dim]Awaiting next consensus epoch...[/dim]")
            
            with Live(table, refresh_per_second=10, console=console) as live:
                try:
                    await asyncio.sleep(15) # Run for 15 seconds to show updates
                except asyncio.CancelledError:
                    pass

            subscribe_task.cancel()
            console.print("\n[bold green]✔ SDK Demo Complete.[/bold green] Agent successfully operated on trusted state.")

    except Exception as e:
        console.print(f"[bold red]Fatal Error:[/bold red] {e}")

if __name__ == "__main__":
    asyncio.run(run_agent_demo())
