using System;
using System.IO;
using UnityEditor;
using UnityEditor.Build;
using UnityEditor.Build.Reporting;
using UnityEditor.SceneManagement;
using UnityEngine;

namespace DofusNativeHarness.Editor
{
    public static class BuildHarness
    {
        private const string ScenePath = "Assets/Scenes/Bootstrap.unity";

        public static void BuildWindowsIl2Cpp()
        {
            Directory.CreateDirectory("Assets/Scenes");
            var scene = EditorSceneManager.NewScene(
                NewSceneSetup.DefaultGameObjects,
                NewSceneMode.Single);
            EditorSceneManager.SaveScene(scene, ScenePath);

            PlayerSettings.productName = "DofusNativeHarness";
            PlayerSettings.companyName = "Nexytrus";
            if (!EditorUserBuildSettings.SwitchActiveBuildTarget(
                    BuildTargetGroup.Standalone,
                    BuildTarget.StandaloneWindows64))
            {
                throw new InvalidOperationException(
                    "Could not activate StandaloneWindows64.");
            }
            PlayerSettings.SetScriptingBackend(
                NamedBuildTarget.Standalone,
                ScriptingImplementation.IL2CPP);

            string projectRoot = Path.GetFullPath(Path.Combine(Application.dataPath, ".."));
            string outputRoot = Path.Combine(projectRoot, "Build", "Windows");
            string executable = Path.Combine(outputRoot, "DofusNativeHarness.exe");
            Directory.CreateDirectory(outputRoot);

            BuildReport report = BuildPipeline.BuildPlayer(new BuildPlayerOptions
            {
                scenes = new[] { ScenePath },
                locationPathName = executable,
                target = BuildTarget.StandaloneWindows64,
                options = BuildOptions.Development
            });
            if (report.summary.result != BuildResult.Succeeded)
                throw new InvalidOperationException(
                    $"Unity build failed: {report.summary.result}");

            string runtime = Path.GetFullPath(Path.Combine(
                projectRoot,
                "..",
                "dist",
                "UnityHarnessRuntime"));
            if (!Directory.Exists(runtime))
                throw new DirectoryNotFoundException(
                    $"Build native runtime first: {runtime}");

            CopyDirectory(runtime, outputRoot);
            Debug.Log($"Harness ready: {executable}");
        }

        private static void CopyDirectory(string source, string destination)
        {
            foreach (string directory in Directory.GetDirectories(
                         source,
                         "*",
                         SearchOption.AllDirectories))
            {
                Directory.CreateDirectory(directory.Replace(source, destination));
            }

            foreach (string file in Directory.GetFiles(
                         source,
                         "*",
                         SearchOption.AllDirectories))
            {
                File.Copy(file, file.Replace(source, destination), true);
            }
        }
    }
}
