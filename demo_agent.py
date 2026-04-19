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
        "[bold white]Verifiable State SDK - Autonomous Agent Interface[/bold white]\n"
        "[dim]Abstracting cryptographic complexity into trusted data objects.[/dim]",
        subtitle="v1.0.0-alpha",
        border_style="bright_blue"
    ))

    try:
        async with VNodeClient(base_url="http://localhost:3000") as sdk:
            
            # --- 1. Verified Query Section ---
            console.print("\n[bold cyan]▶ Requesting Verified Snapshot (Pull Model)[/bold cyan]")
            try:
                # The SDK internally handles fetching the proof, verifying the signatures,
                # and validating the Merkle Inclusion path.
                eth_price = await sdk.get_verified_value("ETH/USD")
                console.print(Panel(
                    f"Asset: [bold]ETH/USD[/bold]\n"
                    f"Value: [bold green]${eth_price:.4f}[/bold]\n"
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
