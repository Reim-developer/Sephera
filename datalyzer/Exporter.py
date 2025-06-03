import os
import sys
import json

try:
    import matplotlib.pyplot as plt
    from rich.console import Console
    from sephera.CodeLoc import CodeLoc
    from tabulate import tabulate
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class Exporter:
    def __init__(self, output_path: str) -> None:
        self.output_path = output_path
        self.console = Console()
    
    def export_file_tree_chart(self, files: int, dirs: int, hidden_files: int, hidden_dirs: int) -> None:
        chart_label: list[str] = ["Files", "Directory", "Hidden Files", "Hidden Directory"]
        chart_values: list[int] = [files, dirs, hidden_files, hidden_dirs]
        colors: list[str] = ["#66b3ff", "#99ff99", "#ffcc99", "#ff9999"]

        _, ax = plt.subplots(figsize = (8, 6)) # type: ignore
        bars = ax.bar(chart_label, chart_values, color = colors, edgecolor = "black") # type: ignore

        with self.console.status("[bold green] Processing...", spinner = "material"):
            for bar in bars: # type: ignore
                bar_height = bar.get_height() # type: ignore
                ax.annotate(f"{bar_height}", xy = (bar.get_x() + bar.get_width() / 2, bar_height), # type: ignore
                xytext = (0, 5),
                textcoords = "offset points",
                ha = "center", va = "bottom", fontsize = 10)
            
            ax.set_title("Sephera Tree Directory Stats", fontsize = 14, fontweight = "bold") # type: ignore
            ax.set_ylabel("Count", fontsize = 12) # type: ignore
            ax.grid(axis = "y", linestyle = "--", alpha = 0.6) # type: ignore

        plt.tight_layout() # type: ignore
        plt.savefig(f"{self.output_path}.png") # type: ignore
        plt.close()

    @staticmethod
    def _autopct(pct: float) -> str:
        return f"{pct:.1f}%" if pct >= 1.0 else ""

    def export_stats_chart(self, data: dict[str, int | float], total_size: float, total_hidden_size: float) -> None:
        chart_colors: list[str] =  ['#ff9999','#66b3ff','#99ff99','#ffcc99']

        threshold_pct: float = 1.0
        total = sum(data.values())
        filter_labels: list[str] = []
        filter_values: list[str] = []
        other_total: float = 0.0

        for label, value in data.items():
            pct = (value / total) * 100
            if pct >= threshold_pct:
                filter_labels.append(label)
                filter_values.append(str(value))
            else:
                other_total += value
        
        if other_total > 0:
            other_pct = (other_total / total) * 100

            filter_labels.append(f"Other: {other_pct:.1f}%")
            filter_values.append(str(other_total))

        fig, ax = plt.subplots(figsize = (8, 8)) # type: ignore
        ax.pie(filter_values, labels = filter_labels, autopct = self._autopct, startangle = 90, colors = chart_colors, pctdistance = 0.85, labeldistance = 1.1)

        centre_circle = plt.Circle((0, 0), 0.70, fc = "white") # type: ignore
        fig.gca().add_artist(centre_circle)

        ax.set_title(label = "Sephera Stats Overview", fontsize = 14) # type: ignore

        plt.figtext(0.5, -0.15, f"Total Size: {total_size / (1024 ** 2):.2f} MB", ha = "center", fontsize = 12) # type: ignore
        plt.figtext(0.5, -0.20, f"Total Hidden Size: {total_hidden_size / (1024 ** 2):.2f} MB", ha = "center", fontsize = 12) # type: ignore

        plt.savefig(f"{self.output_path}.png", bbox_inches = "tight") # type: ignore
        plt.close()


    def export_to_markdown(self, file_path: str, codeLoc: CodeLoc) -> None:
        total_loc_count: int = 0
        total_comment: int = 0
        total_empty: int = 0
        total_project_size: float = 0.0
        language_count: int = 0

        markdown_data = []
        markdown_headers = ["Language", "Code lines", "Comment lines", "Empty lines", "Size (MB)"]

        for language, count in codeLoc.loc_count.items():
            loc_line = count["loc"]
            comment_line = count["comment"]
            empty_line = count["empty"]
            total_sizeof = count["size"]

            if loc_line > 0 or comment_line > 0 or empty_line > 0 or total_sizeof > 0:

                language_count += 1
                language_config  = codeLoc.language_data.get_language_by_name(name = language)

                comment_result = (
                    "N/A" if language_config.comment_style == "no_comment" # type: ignore
                    else str(comment_line)
                )

                markdown_data.append([ # type: ignore
                    language, loc_line, comment_result,
                    empty_line, f"{total_sizeof:.2f}"
                ])

                total_loc_count += loc_line # type: ignore
                total_comment += comment_line # type: ignore
                total_empty += empty_line # type: ignore
                total_project_size += total_sizeof

               

        markdown_table = tabulate(
            markdown_data, headers = markdown_headers, # type: ignore
            tablefmt = "github", numalign = "right",
            stralign = "left",
            missingval = "N/A"
        )

        markdown_source: list[str] = []
        markdown_source.append(f"# LOC Count of directory: {os.path.abspath(codeLoc.base_path)}\n")
        markdown_source.append(markdown_table)
        markdown_source.append("\n## Project Total:")
        markdown_source.append(f"- **Code**: {total_loc_count} lines")
        markdown_source.append(f"- **Comments**: {total_comment} lines")
        markdown_source.append(f"- **Empty**: {total_empty} lines")
        markdown_source.append(f"- **Language(s) used**: {language_count} language(s)")
        markdown_source.append(f"- **Total Project Size**: {total_project_size:.2f} MB")

        with open(file = file_path, mode = "w", encoding = "utf-8") as markdown:
            markdown.write("\n".join(markdown_source))


    def export_to_json(self, file_path: str, codeLoc: CodeLoc) -> None:
        total_loc_count: int = 0
        total_comment: int = 0
        total_empty: int = 0
        total_project_size: float = 0.0
        language_count: int = 0

        finish_result = {}
        for language, count in codeLoc.loc_count.items():
            loc_line = count["loc"]
            comment_line = count["comment"]
            empty_line = count["empty"]
            total_sizeof = count["size"]

            if loc_line > 0 or comment_line > 0 or empty_line > 0 or total_sizeof > 0:

                language_count += 1
                language_config  = codeLoc.language_data.get_language_by_name(name = language)

                comment_result = (
                    "N/A" if language_config.comment_style == "no_comment" # type: ignore
                    else str(comment_line)
                )

                total_loc_count += loc_line # type: ignore
                total_comment += comment_line # type: ignore
                total_empty += empty_line # type: ignore
                total_project_size += total_sizeof

                finish_result[language] = {
                        "Code lines": loc_line,
                        "Comment lines": comment_result,
                        "Empty lines": empty_line,
                        "Size (MB)": f"{total_sizeof:.2f} MB" 
                }

        total_data: dict[str, object] = {
            "Scan In:": f"{os.path.abspath(path = codeLoc.base_path)}",
                "Total:": {
                    "Language(s) used:": language_count,
                    "Code lines": total_loc_count,
                    "Comment": total_comment,
                    "Empty": total_empty,
                    "Project Size (MB):": f"{total_project_size:.2f} MB"
            }
        }
        
        finish_result: dict[object, object] = {**total_data, **finish_result}
        with open(file = file_path, mode = "w", encoding = "utf-8") as json_file:
            json.dump(finish_result, json_file, indent = 4, ensure_ascii = True)