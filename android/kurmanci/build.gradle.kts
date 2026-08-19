plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    `maven-publish`
    signing
}

val kurmanciMavenGroup = project.findProperty("kurmanciMavenGroup")?.toString() ?: "io.github.ferhatguneri"
val kurmanciVersion = project.findProperty("kurmanciVersion")?.toString() ?: "0.1.0"
val centralRelease = (project.findProperty("centralRelease")?.toString() ?: "false").toBoolean()

android {
    namespace = "org.kurmanci"
    compileSdk = 34

    defaultConfig {
        minSdk = 23
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    publishing {
        singleVariant("release") {
            withSourcesJar()
            withJavadocJar()
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
}

publishing {
    publications {
        register<MavenPublication>("release") {
            groupId = kurmanciMavenGroup
            artifactId = "kurmanci-android"
            version = kurmanciVersion

            afterEvaluate {
                from(components["release"])
            }

            pom {
                name.set("Kurmancî Android SDK")
                description.set("A production-oriented offline Kurmancî language SDK for Android, providing spell checking, correction, completion and next-word prediction through the Kurmancî engine.")
                url.set("https://github.com/Kurdi-Language/kurmanci")
                licenses {
                    license {
                        name.set("The Apache Software License, Version 2.0")
                        url.set("https://www.apache.org/licenses/LICENSE-2.0.txt")
                        distribution.set("repo")
                    }
                }
                developers {
                    developer {
                        id.set("Kurdi-Language")
                        name.set("Kurdi-Language Organization Contributors")
                        url.set("https://github.com/Kurdi-Language/kurmanci")
                    }
                }
                scm {
                    connection.set("scm:git:https://github.com/Kurdi-Language/kurmanci.git")
                    developerConnection.set("scm:git:ssh://git@github.com/Kurdi-Language/kurmanci.git")
                    url.set("https://github.com/Kurdi-Language/kurmanci")
                }
            }
        }
    }
    repositories {
        maven {
            name = "distMaven"
            url = uri(project.rootProject.layout.projectDirectory.dir("../dist/android/maven"))
        }
        maven {
            name = "centralStagingMaven"
            url = uri(project.rootProject.layout.projectDirectory.dir("../dist/android/central-staging"))
        }
    }
}

signing {
    val signingKey = System.getenv("MAVEN_SIGNING_KEY") ?: project.findProperty("signingKey")?.toString()
    val signingPassword = System.getenv("MAVEN_SIGNING_PASSWORD") ?: project.findProperty("signingPassword")?.toString()

    if (centralRelease) {
        if (signingKey.isNullOrBlank() || signingPassword.isNullOrBlank()) {
            error("❌ Mandatory release error: centralRelease is true but MAVEN_SIGNING_KEY or MAVEN_SIGNING_PASSWORD is not set!")
        }
        useInMemoryPgpKeys(signingKey, signingPassword)
        sign(publishing.publications["release"])
    } else if (!signingKey.isNullOrBlank() && !signingPassword.isNullOrBlank()) {
        useInMemoryPgpKeys(signingKey, signingPassword)
        sign(publishing.publications["release"])
    }
}
