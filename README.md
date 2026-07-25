# Dofus 3 Native Host

Runtime de mods natifs x64 pour Dofus 3 sous Unity IL2CPP, sans MelonLoader.

Le projet fournit :

- un bootstrap `version.dll` chargé naturellement par Unity ;
- un host de mods natifs avec une ABI C versionnée ;
- un SDK Rust partagé pour développer des mods sans recopier la logique IL2CPP ;
- un resolver global de classes, champs et méthodes par signatures structurelles ;
- un canal de contrôle permettant de charger, décharger et recharger une DLL ;
- le mod **Dofus 3 Native Tactical**, activable avec `F8`.

> [!IMPORTANT]
> Ce projet n’est ni développé, ni approuvé, ni distribué par Ankama. Une mise
> à jour de Dofus peut casser la compatibilité ou remplacer le `version.dll`
> installé. Utilisez-le à vos risques et périls.

## Installation rapide

### 1. Télécharger l’installateur

Téléchargez `Dofus3NativeHost-Setup-windows-x64.exe` depuis la
[dernière release](https://github.com/AlpaGit/dofus3-native-host/releases/latest).

### 2. Fermer Dofus

Fermez toutes les fenêtres Dofus avant de remplacer les DLL. Le launcher Ankama
peut rester ouvert.

### 3. Choisir le jeu

Lancez l’installateur. Il propose le chemin Ankama habituel et permet d’en
choisir un autre. Sélectionnez le dossier qui contient `Dofus.exe` ;
l’installateur le vérifie avant de continuer.

L’installateur communautaire n’est pas encore signé avec un certificat de
signature de code. Windows peut donc afficher SmartScreen ; vérifiez le
SHA-256 publié dans la release avant de l’exécuter.

L’emplacement habituel est :

```text
C:\Users\<vous>\AppData\Local\Ankama\Dofus-dofus3
```

Après installation, le dossier doit ressembler à ceci :

```text
Dofus-dofus3/
  Dofus.exe
  version.dll
  DofusNativeBootstrap.dll
  DofusNativeHost.dll
  NativeMods/
    DofusNativeExample.dll
    DofusNativeTactical.dll
```

### 4. Lancer

Lancez Dofus normalement depuis l’Ankama Launcher.

Sur une carte, appuyez sur `F8` pour activer ou désactiver le mode tactique
natif. Le mod génère les 560 cellules dans un seul mesh Unity.

### Installation portable

La release contient aussi `Dofus3NativeHost-windows-x64.zip`. Pour une
installation manuelle, extrayez son contenu directement à côté de `Dofus.exe`.

## Mettre à jour

1. Fermez Dofus.
2. Téléchargez et relancez le nouvel installateur.
3. Sélectionnez le même dossier de jeu ; les fichiers sont remplacés.

Après certaines mises à jour du jeu, Ankama peut supprimer ou remplacer le
`version.dll` local. Il suffit alors de réinstaller la dernière release.

## Désinstaller

Utilisez **Applications installées > Dofus 3 Native Host > Désinstaller**.
L’installateur retire uniquement ses propres DLL et conserve les autres mods.

Pour une désinstallation manuelle, fermez Dofus puis supprimez uniquement :

```text
version.dll
DofusNativeBootstrap.dll
DofusNativeHost.dll
NativeMods/DofusNativeExample.dll
NativeMods/DofusNativeTactical.dll
NativeMods/native-control.json
```

Les journaux et dossiers `NativeMods/control` peuvent également être supprimés.
Ne supprimez pas tout le dossier `NativeMods` si vous y avez ajouté vos propres
mods.

## Diagnostic

Les journaux se trouvent dans :

```text
NativeMods/native-bootstrap.log
NativeMods/native-host.log
```

Un démarrage sain contient notamment :

```text
Dofus Native Host ABI v4 starting
Dofus 3 Native Tactical negotiated native ABI v4
Ready. Press F8 to toggle native tactical mode (Rust SDK, pointer-safe ABI v4).
```

Si `F8` ne fait rien :

1. vérifiez que les cinq DLL ci-dessus sont au bon endroit ;
2. vérifiez `NativeMods/native-host.log` ;
3. réinstallez après toute mise à jour récente de Dofus ;
4. ouvrez une issue avec le journal, la version du jeu et les étapes exactes.

## Pourquoi une ABI v4 ?

Dofus 3 utilise Unity 6 et IL2CPP. Les noms du code du jeu peuvent être
obfusqués et changer entre deux builds. L’ABI évite donc de dépendre uniquement
de noms fragiles :

- résolution de classes, champs et méthodes par type et signature ;
- inspection des classes génériques construites ;
- invocation IL2CPP et remontée des exceptions ;
- création d’objets, chaînes et tableaux ;
- lecture et écriture de champs ;
- `GCHandle` natifs de taille pointeur.

Unity 6 utilise ici des handles 64 bits. L’ABI v4 les conserve sans troncature.
Les anciennes tables v2/v3 restent prises en charge au moyen de jetons 32 bits
gérés par le host.

## Développer un mod en Rust

Le workspace est séparé en trois couches :

```text
crates/mod-api   contrat FFI brut, stable et sans dépendance
crates/mod-sdk   API Rust commune et réutilisable
crates/host      chargeur, contrôle et pont UnityResolve
mods/*           DLL de mods
```

Un mod Rust est une bibliothèque `cdylib` qui exporte :

- `DNM_Query` pour négocier l’ABI et annoncer ses métadonnées ;
- `DNM_Load` pour initialiser le mod ;
- `DNM_Tick` appelé chaque frame sur le thread Unity ;
- `DNM_Unload` pour restaurer et libérer toutes les ressources.

Le SDK fournit notamment :

```rust
let runtime = unsafe { Runtime::bind(host_api) }.ok_or(DNH_ERROR)?;

let map_renderer = runtime.class(
    c"Core.dll",
    c"Core.Rendering",
    c"MapRenderer",
);

let map_id_fields =
    runtime.fields_by_type(map_renderer, c"System.Int64");

let method = runtime.instance_method(
    some_class,
    c"set_enabled",
    &[c"System.Boolean"],
);
```

`mods/native-tactical` est l’exemple complet recommandé. Sa logique générique
de signatures, d’invocation, d’unboxing, de tableaux, de logs et de handles se
trouve dans `crates/mod-sdk`, afin que les prochains mods ne la dupliquent pas.

## Compiler

Prérequis :

- Windows x64 ;
- Rust stable avec la cible `x86_64-pc-windows-msvc` ;
- Visual Studio 2022 Build Tools avec **Desktop development with C++** ;
- PowerShell.

```powershell
git clone https://github.com/AlpaGit/dofus3-native-host.git
cd dofus3-native-host
.\build.ps1 -Configuration Release
```

Le paquet installable est généré dans :

```text
dist/DofusRuntime/
```

Contrôles de qualité :

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Harness Unity

`unity-harness` est un projet Unity 6 contrôlé qui valide la chaîne :

```text
version.dll
  -> DofusNativeBootstrap.dll
  -> DofusNativeHost.dll
  -> NativeMods/*.dll
```

Avec Unity 6.0 LTS et Windows IL2CPP Build Support installés :

```powershell
.\build-unity-harness.ps1
```

Le player est produit dans
`unity-harness/Build/Windows/DofusNativeHarness.exe`.

## Contrôle externe et MCP

À chaque démarrage, le host publie :

```text
NativeMods/native-control.json
```

Le descripteur pointe vers une boîte aux lettres locale traitée sur le thread
Unity. Les commandes disponibles sont :

- `STATUS` ;
- `LIST` ;
- `LOAD <chemin absolu de DLL>` ;
- `UNLOAD <id du mod>` ;
- `RELOAD <id du mod> <chemin absolu de DLL>`.

Cette surface permet à un outil MCP ou à un environnement de développement de
piloter les DLL sans injecteur supplémentaire. Le protocole reste local à la
session Dofus et les chemins sont validés par le host.

## Sécurité

Une DLL de mod s’exécute dans le processus Dofus et possède les mêmes droits que
le jeu. N’installez que des mods dont vous connaissez la provenance. Le host
valide le contrat ABI, mais ne constitue pas une sandbox.

## Licence

Le code du projet est distribué sous licence MIT. UnityResolve est inclus sous
sa propre licence MIT, conservée dans `third_party/unityresolve/LICENSE`.
