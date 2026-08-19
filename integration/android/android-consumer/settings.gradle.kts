pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

val consumerMode =
    System.getenv("CONSUMER_MODE")
        ?: providers.gradleProperty("consumerMode").orNull
        ?: "local"

val stagedRepoUrl =
    System.getenv("STAGED_REPO_URL")
        ?: providers.gradleProperty("stagedRepoUrl").orNull
        ?: ""

val stagedAuthToken =
    System.getenv("STAGED_AUTH_TOKEN")
        ?: providers.gradleProperty("stagedAuthToken").orNull
        ?: ""

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        when (consumerMode) {
            "local" -> {
                maven {
                    name = "localDist"
                    url = uri(rootDir.resolve("../../../dist/android/maven"))
                }
                google()
                mavenCentral()
            }
            "staged" -> {
                if (stagedRepoUrl.isBlank()) {
                    error("❌ CONSUMER_MODE=staged requires STAGED_REPO_URL environment variable or stagedRepoUrl property.")
                }
                if (stagedAuthToken.isBlank()) {
                    error("❌ CONSUMER_MODE=staged requires STAGED_AUTH_TOKEN environment variable or stagedAuthToken property.")
                }
                maven {
                    name = "stagedCentral"
                    url = uri(stagedRepoUrl)
                    credentials(HttpHeaderCredentials::class) {
                        name = "Authorization"
                        value = if (stagedAuthToken.startsWith("Bearer ") || stagedAuthToken.startsWith("Basic ")) stagedAuthToken else "Bearer $stagedAuthToken"
                    }
                    authentication {
                        create<HttpHeaderAuthentication>("header")
                    }
                }
                google()
                mavenCentral()
            }
            "public" -> {
                google()
                mavenCentral()
            }
            else -> {
                error("Unknown CONSUMER_MODE: '$consumerMode'. Allowed modes are 'local', 'staged', 'public'.")
            }
        }
    }
}

rootProject.name = "android-consumer"
include(":app")
