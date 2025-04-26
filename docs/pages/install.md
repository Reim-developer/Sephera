# **Sephera Install**

## Manual installation:
---
**Install via Github page:**

* Sephera can be install lastest verson in [Github release page.](https://github.com/Reim-developer/Sephera/releases)
* You just download the binary of your operating system, and run it. No setup requirements. But add Sephera to your PATH environment is recommended to use everywhere.
> **This install method is recommended for new users.**

## Advanced installation:
---
**Install via cURL or wget (Linux/macOS):**

* With `cURl`, put this command to your terminal:
```bash
curl -sSL https://raw.githubusercontent.com/Reim-developer/Sephera/master/install.sh | bash
```
---
* With `wget`, put this command to your terminal
```bash
wget -qO- https://raw.githubusercontent.com/Reim-developer/Sephera/master/install.sh | bash
```
---
**In Windows, you can only install via [Github release page](https://github.com/Reim-developer/Sephera/releases), or [build from source](#build-from-source)**

## Build from source:
---
**Requirements:**

* Python >= `3.13` or above.
* Make >= `4.4.1` or above if you wish build with `Makefile`.
* Git >= `2.49.0` or above to clone Sephera source.
---

*1: Clone Sephera repo:*
```bash
git clone https://github.com/reim-developer/Sephera && cd Sephera
```
---
*2: Install dependencies from requiremens.txt:*
```bash
pip install -r requirements.txt
```
---
*3: Now, you can build Sephera:*

* If you wish build with `Make`, run this:
```bash
make build
```
---
* Or you can build with Shell script (Linux or macOS):
```bash
chmod +x ./build.sh && ./build.sh
```
---
* You can also use Sephera without build:
```bash
python main.py help 
```
