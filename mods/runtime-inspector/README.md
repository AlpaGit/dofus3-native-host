# Dofus Runtime Inspector

Portage Rust de la partie inspection visuelle de l'ancien
`Dofus3RuntimeDumper`.

- `F9` démarre un snapshot progressif afin de ne pas bloquer le jeu pendant une
  frame complète.
- Les renderers, matériaux, sprites, meshes et objets `Moquette` sont indexés
  dans des CSV.
- Les textures référencées sont copiées en PNG quand l'API Unity le permet.
- Les sorties restent locales dans `NativeMods/DofusRuntimeDump/<session>/`.

La capture réseau appartient au mod séparé `Dofus Network Dumper`.
