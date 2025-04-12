import matplotlib.pyplot as plt
from typing import Callable

class Exporter:
    def __init__(self, output_path: str) -> None:
        self.output_path = output_path
    
    """"

    Export file tree from tree command to chart

    """
    def export_file_tree_chart(self, files: int, dirs: int, hidden_files: int, hidden_dirs: int, on_step: Callable[[], None] = None) -> None:
        chart_label: list[str] = ["Files", "Directory", "Hidden Files", "Hidden Directory"]
        chart_values: list[int] = [files, dirs, hidden_files, hidden_dirs]
        colors: list[str] = ["#66b3ff", "#99ff99", "#ffcc99", "#ff9999"]

        _, ax = plt.subplots(figsize = (8, 6))
        on_step()

        bars = ax.bar(chart_label, chart_values, color = colors, edgecolor = "black")
        on_step()

        for bar in bars:
            bar_height = bar.get_height()
            ax.annotate(f"{bar_height}", xy = (bar.get_x() + bar.get_width() / 2, bar_height),
            xytext = (0, 5),
            textcoords = "offset points",
            ha = "center", va = "bottom", fontsize = 10)

        on_step()
        
        ax.set_title("Sephera Tree Directory Stats", fontsize = 14, fontweight = "bold")
        ax.set_ylabel("Count", fontsize = 12)
        ax.grid(axis = "y", linestyle = "--", alpha = 0.6)

        plt.tight_layout()
        plt.savefig(f"{self.output_path}.png")
        plt.close()
        on_step()

    def export_stats_chart(self, data: dict, total_size: float, total_hidden_size: float) -> None:
        chart_label = list(data.keys())
        chart_values = list(data.values())
        chart_colors: list[str] =  ['#ff9999','#66b3ff','#99ff99','#ffcc99']

        fig, ax = plt.subplots(figsize = (8, 8))
        ax.pie(chart_values, labels = chart_label, autopct = "%1.1f%%", startangle = 90, colors = chart_colors, pctdistance = 0.85)

        centre_circle = plt.Circle((0, 0), 0.70, fc = "white")
        fig.gca().add_artist(centre_circle)

        ax.set_title("Sephera Stats Overview", fontsize = 14)

        plt.figtext(0.5, -0.15, f"Total Size: {total_size / (1024 ** 2):.2f} MB", ha = "center", fontsize = 12)
        plt.figtext(0.5, -0.20, f"Total Hidden Size: {total_hidden_size / (1024 ** 2):.2f} MB", ha = "center", fontsize = 12)

        plt.savefig(f"{self.output_path}.png", bbox_inches = "tight")
        plt.close()
