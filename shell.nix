{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
    buildInputs = [
        (pkgs.python312.withPackages (ps: with ps; [
            matplotlib
            rich
        ]))
    ];
}
