node {

    @Library("kenbun_pipeline")_

    checkout scm

    generalPipeline{
        def product = 'kidou'
        def name = 'cpal'
        if (env.BRANCH_NAME == 'master') {
            build_and_publish()
        } else {
            build_and_lint()
        }
    }
}

def latest_published_version(String pkg) {
    sh(returnStdout: true, script: 'cargo search -q "' + pkg + '" --registry kenbun | sed \'s/' + pkg + ' = "\\(.*\\)".*/\\1/\'')
}

def current_version(String pkg) {
    sh(returnStdout: true, script: 'cargo pkgid -p "' + pkg + '" | sed \'s/.*[@:#]\\(.*\\)/\\1/\'')
}

def version_changed(String pkg) {
    latest_published_version(pkg) != current_version(pkg)
}

def publish_if_updated(String pkg) {
    if (version_changed(pkg)) {
      echo 'publishing new version of ' + pkg
      sh('cargo publish -p "' + pkg + '" --registry=kenbun')
    } else {
      echo 'Version of ' + pkg + ' did not change, not publishing...'
    }
}

def build_and_publish() {
    stage("build and publish") {
        docker.withRegistry(env.nexusDockerRepo, 'nexus') {
            def rustImage = docker.image('docker.kenbun.de/kenbun/rust-build-container-cpp:2.0.0-ubuntu20.04-rust1.65')
            rustImage.pull()
            withCredentials([string(credentialsId: 'meuse-api-key', variable: 'meuse_api_key')]) {
                rustImage.withRun() { c ->
                    rustImage.inside("--env CARGO_REGISTRIES_KENBUN_TOKEN=${meuse_api_key} --env CARGO_REGISTRIES_KENBUN_INDEX=ssh://git@github.com/kenbunitag/rust-registry.git -v " + env.WORKSPACE.toString() + ":/io --network='host'") {
                        withCredentials([
                            usernamePassword(credentialsId: 'nexus', usernameVariable: 'nexusUser', passwordVariable: 'nexusPassword'),
                            sshUserPrivateKey(credentialsId: 'github-ssh', keyFileVariable: 'ssh_identity', usernameVariable: 'ssh_username'),
                            file(credentialsId: 'github-ssh-password', variable: 'github_ssh_password')
                        ]) {
                            sshagent(credentials: ['github-ssh']) {
                                sh("./build_and_test.sh")
                                publish_if_updated('cpal')
                            }
                        }
                    }
                }
            }
        }
    }
}



def build_and_lint() {
    stage("build and test") {
        docker.withRegistry(env.nexusDockerRepo, 'nexus') {
            def rustImage = docker.image('docker.kenbun.de/kenbun/rust-build-container-cpp:2.0.0-ubuntu20.04-rust1.65')
            rustImage.pull()
            withCredentials([string(credentialsId: 'meuse-api-key', variable: 'meuse_api_key')]) {
                rustImage.withRun() { c ->
                    rustImage.inside("--env CARGO_REGISTRIES_KENBUN_TOKEN=${meuse_api_key} --env CARGO_REGISTRIES_KENBUN_INDEX=ssh://git@github.com/kenbunitag/rust-registry.git -v " + env.WORKSPACE.toString() + ":/io --network='host'") {
                        withCredentials([
                            sshUserPrivateKey(credentialsId: 'github-ssh', keyFileVariable: 'ssh_identity', usernameVariable: 'ssh_username'),
                            file(credentialsId: 'github-ssh-password', variable: 'github_ssh_password')
                        ]) {
                            sshagent(credentials: ['github-ssh']) {
                                sh("./build_and_test.sh")
                            }
                        }
                    }
                }
            }
        }
    }
}
