# Dofus Native Bootstrap

Bootstrap Windows x64 pour Dofus 3.
Le même binaire est distribué sous deux noms :

- `version.dll`, chargé par `UnityPlayer.dll` et chargé de transmettre l'API
  Windows Version vers `System32/version.dll` ;
- `DofusNativeBootstrap.dll`, cible stable de l'API explicite `DNB_*`.

Dans `Dofus.exe`, un worker attend `GameAssembly.dll`, un domaine IL2CPP valide
et la fenêtre `UnityWndClass`. Il installe ensuite un hook `WH_GETMESSAGE` limité
au thread de cette fenêtre. L'initialisation et les ticks du host s'exécutent
ainsi sur le thread Unity sans patcher le code du jeu.

Le bootstrap refuse de charger le host en dehors du couple
`Dofus.exe` + `Dofus_Data`. Les erreurs sont écrites dans
`NativeMods/native-bootstrap.log`.
