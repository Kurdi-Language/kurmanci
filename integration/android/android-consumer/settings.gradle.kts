pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}
dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
        maven {
            name = "localDist"
            url = uri(rootDir.resolve("../../../dist/android/maven"))
        }
    }
}

rootProject.name = "android-consumer"
include(":app")
