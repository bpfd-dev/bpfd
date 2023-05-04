# bpfd-operator

The Bpfd-Operator repository exists in order to deploy and manage bpfd within a kubernetes cluster.

## Getting started

This operator was built utilizing some great tooling provided by the [operator-sdk library](https://sdk.operatorframework.io/). A great first step in understanding some
of the functionality can be to just run `make help`.

### Deploy locally via KIND

After reviewing the possible make targets it's quick and easy to get bpfd deployed locally on your system via a [KIND cluster](https://kind.sigs.k8s.io/). with:

```bash
make run-on-kind
```

### Deploy To Openshift Cluster

First install cert-manager (if not already deployed) to the cluster with:

```bash
make deploy-cert-manager
```

Then deploy the operator with one of the following two options:

#### 1. Manually with Kustomize

Then to install manually with Kustomize and raw manifests simply run:

```bash
make deploy-openshift
```

Which can then be cleaned up with:

```bash
make undeploy-openshift
```

#### 2. Via the OLM bundle

The bpfd-operator can also be installed via it's [OLM bundle](https://www.redhat.com/en/blog/deploying-operators-olm-bundles).

First setup the namespace and certificates for the operator with:

```bash
oc apply -f ./hack/ocp-scc-hacks.yaml
```

Then use `operator-sdk` to install the bundle like so:

```bash
operator-sdk run bundle quay.io/bpfd/bpfd-operator-bundle:latest --namespace openshift-bpfd
```

To clean everything up run:

```bash
operator-sdk cleanup bpfd-operator
```

followed by

```bash
oc delete -f ./hack/ocp-scc-hacks.yaml
```

### Verify the installation

If the bpfd-operator came up successfully you will see the bpfd-daemon and bpfd-operator pods running without errors:

```bash
kubectl get pods -n bpfd
NAME                             READY   STATUS    RESTARTS   AGE
bpfd-daemon-bt5xm                2/2     Running   0          130m
bpfd-daemon-ts7dr                2/2     Running   0          129m
bpfd-daemon-w24pr                2/2     Running   0          130m
bpfd-operator-78cf9c44c6-rv7f2   2/2     Running   0          132m
```

### Deploy a bpf Program to the cluster

To test the deployment simply deploy one of the sample `xdpPrograms`:

```bash
kubectl apply -f config/samples/bpfd.io_v1alpha1_xdp_pass_xdpprogram.yaml
```

If loading of the Xdp Program to the selected nodes was successful it will be reported
back to the user via the `xdpProgram`'s status field:

```bash
kubectl get xdpprogram xdp-pass-all-nodes -o yaml
apiVersion: bpfd.io/v1alpha1
  kind: XdpProgram
  metadata:
    creationTimestamp: "2023-05-04T18:28:46Z"
    finalizers:
    - bpfd.io.operator/finalizer
    generation: 1
    labels:
      app.kubernetes.io/name: xdpprogram
    name: xdp-pass-all-nodes
    resourceVersion: "11205"
    uid: 8246b56c-b78e-43fc-bb78-3b46b1490a0c
  spec:
    bytecode:
      image:
        imagepullpolicy: IfNotPresent
        url: quay.io/bpfd-bytecode/xdp_pass:latest
    interfaceselector:
      primarynodeinterface: true
    nodeselector: {}
    priority: 0
    proceedon:
    - pass
    - dispatcher_return
    sectionname: pass
  status:
    conditions:
    - lastTransitionTime: "2023-05-04T18:28:46Z"
      message: bpfProgramReconciliation Succeeded on all nodes
      reason: ReconcileSuccess
      status: "True"
      type: ReconcileSuccess
kind: List
metadata:
  resourceVersion: ""
```

To see more information in listing form simply run:

```bash
kubectl get xdpprogram -o wide
NAME                 SECTIONNAME   BYTECODE                                                                                     NODESELECTOR   PRIORITY   INTERFACESELECTOR               PROCEEDON
xdp-pass-all-nodes   pass          {"image":{"imagepullpolicy":"IfNotPresent","url":"quay.io/bpfd-bytecode/xdp_pass:latest"}}   {}             0          {"primarynodeinterface":true}   ["pass","dispatcher_return"]
```

### API Types Overview

#### BpfProgramConfig

The multiple `*Program` crds are the bpfd K8s API objects most relevant to users and can be used to understand clusterwide state for an ebpf program. It's designed to express how, and where bpf programs are to be deployed within a kubernetes cluster. Currently bpfd supports the use of `xdpPrograms`, `tcPrograms` and `tracepointProgram` objects.

### BpfProgram

The `BpfProgram` crd is used internally by the bpfd-deployment to keep track of per node bpfd state such as map pin points, and to report node specific errors back to the user. K8s users/controllers are only allowed to view these objects, NOT create or edit them.

Applications wishing to use bpfd to deploy/manage their bpf programs in kubernetes will make use of this
object to find references to the bpfMap pin points (`spec.maps`) in order to configure their bpf programs.

## Contributing
// TODO(astoycos): Add detailed information on how you would like others to contribute to this project

## License

Copyright 2022.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
