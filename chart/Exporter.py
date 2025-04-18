import sys
import time

try:
    import matplotlib.pyplot as plt
    from rich.console import Console
except KeyboardInterrupt:
    print("\n Aborted by user.")
    sys.exit(1)

class Exporter:
    def __init__(self, output_path: str) -> None:
        self.output_path = output_path
        self.console = Console()
    
    def export_file_tree_chart(self, files: int, dirs: int, hidden_files: int, hidden_dirs: int) -> None:
        chart_label: list[str] = ["Files", "Directory", "Hidden Files", "Hidden Directory"]
        chart_values: list[int] = [files, dirs, hidden_files, hidden_dirs]
        colors: list[str] = ["#66b3ff", "#99ff99", "#ffcc99", "#ff9999"]

        _, ax = plt.subplots(figsize = (8, 6))
        bars = ax.bar(chart_label, chart_values, color = colors, edgecolor = "black")

        with self.console.status("[bold green] Processing...", spinner = "point") as progressBar:
            for bar in bars:
                bar_height = bar.get_height()
                ax.annotate(f"{bar_height}", xy = (bar.get_x() + bar.get_width() / 2, bar_height),
                xytext = (0, 5),
                textcoords = "offset points",
                ha = "center", va = "bottom", fontsize = 10)
            
            ax.set_title("Sephera Tree Directory Stats", fontsize = 14, fontweight = "bold")
            ax.set_ylabel("Count", fontsize = 12)
            ax.grid(axis = "y", linestyle = "--", alpha = 0.6)

        plt.tight_layout()
        plt.savefig(f"{self.output_path}.png")
        plt.close()

    @staticmethod
    def _autopct(pct: float) -> str:
        return f"{pct:.1f}%" if pct >= 1.0 else ""

    def export_stats_chart(self, data: dict, total_size: float, total_hidden_size: float) -> None:
        chart_colors: list[str] =  ['#ff9999','#66b3ff','#99ff99','#ffcc99']

        threshold_pct: float = 1.0
        total = sum(data.values())
        filter_labels: list = []
        filter_values: list = []
        other_total: float = 0.0

        for label, value in data.items():
            pct = (value / total) * 100
            if pct >= threshold_pct:
                filter_labels.append(label)
                filter_values.append(value)
            else:
                other_total += value
        
        if other_total > 0:
            other_pct = (other_total / total) * 100

            filter_labels.append(f"Other: {other_pct:.1f}%")
            filter_values.append(other_total)

        fig, ax = plt.subplots(figsize = (8, 8))
        ax.pie(filter_values, labels = filter_labels, autopct = self._autopct, startangle = 90, colors = chart_colors, pctdistance = 0.85, labeldistance = 1.1)

        centre_circle = plt.Circle((0, 0), 0.70, fc = "white")
        fig.gca().add_artist(centre_circle)

        ax.set_title("Sephera Stats Overview", fontsize = 14)

        plt.figtext(0.5, -0.15, f"Total Size: {total_size / (1024 ** 2):.2f} MB", ha = "center", fontsize = 12)
        plt.figtext(0.5, -0.20, f"Total Hidden Size: {total_hidden_size / (1024 ** 2):.2f} MB", ha = "center", fontsize = 12)

        plt.savefig(f"{self.output_path}.png", bbox_inches = "tight")
        plt.close()
